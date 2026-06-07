#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_SCRIPT_SURFACE_AUDIT_ROOT:-$ROOT/target/script-surface-audit/latest}"
SUMMARY="$OUT_ROOT/summary.json"
REPORT="$OUT_ROOT/classification-report.md"
SNAPSHOT_MANIFEST="$OUT_ROOT/snapshot-manifest.json"

# shellcheck source=scripts/lib/pulse-gate-lib.sh
source "$ROOT/scripts/lib/pulse-gate-lib.sh"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

ao2_gate_forbidden_string_scan "$OUT_ROOT/logs" "$ROOT/scripts"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$REPORT" "$SNAPSHOT_MANIFEST" <<'PY'
import hashlib
import json
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()
snapshot_manifest_path = Path(sys.argv[5]).resolve()
snapshot_scripts = out_root / "snapshot" / "scripts"
snapshot_scripts.mkdir(parents=True, exist_ok=True)

package = json.loads((root / "package.json").read_text(encoding="utf-8"))
package_scripts = set(package.get("scripts", {}))

status_output = subprocess.check_output(
    ["git", "status", "--porcelain=v1", "--untracked-files=all", "--", "scripts"],
    cwd=root,
    text=True,
)
untracked_scripts = sorted(
    line[3:]
    for line in status_output.splitlines()
    if line.startswith("?? ")
    and line.endswith(".sh")
    and line[3:] != "scripts/script-surface-audit.sh"
)

token_openai = "OPENAI" + "_API_KEY"
token_anthropic = "ANTHROPIC" + "_API_KEY"
token_private_repo = "ao2-control-plane" + "-private"
token_local_api = "target/long-lived-control-plane" + "/api-token"
token_private_dir = "/Documents/" + "private"
forbidden_patterns = [
    {"pattern": token_openai, "category": "provider_api_key"},
    {"pattern": token_anthropic, "category": "provider_api_key"},
    {"pattern": token_private_repo, "category": "secret_path"},
    {"pattern": token_local_api, "category": "secret_path"},
    {"pattern": token_private_dir, "category": "secret_path"},
    {"pattern": "gh release" + " create", "category": "publishing_side_effect"},
    {"pattern": "git push" + " origin", "category": "publishing_side_effect"},
    {"pattern": "npm" + " publish", "category": "publishing_side_effect"},
]


def npm_commands(text):
    commands = set()
    for match in re.finditer(r"\bnpm\s+run\s+([A-Za-z0-9:_./-]+)", text):
        command = match.group(1).rstrip(";,)")
        if command and command != "--":
            commands.add(command)
    return sorted(commands)


def classify(script_name, missing_commands):
    if script_name.startswith("control-plane-") or script_name.startswith("operator-readiness-"):
        return "defer_control_plane"
    if "lengthy-gate" in script_name or script_name.startswith("release-helper-migration-"):
        return "consolidate"
    if script_name.startswith("script-tracking-"):
        return "promote_candidates"
    if "shared-gate-library" in script_name:
        return "promote_candidates"
    if script_name.startswith("public-hardening-"):
        return "promote_candidates"
    if missing_commands:
        return "local_only"
    return "local_only"


items = []
buckets = {
    "promote_candidates": [],
    "local_only": [],
    "consolidate": [],
    "defer_control_plane": [],
    "remove_later": [],
}
missing_package_commands = []
forbidden_reference_hits = []

for rel_path in untracked_scripts:
    source_path = root / rel_path
    data = source_path.read_bytes()
    text = data.decode("utf-8", errors="replace")
    script_name = source_path.name
    destination = snapshot_scripts / script_name
    shutil.copy2(source_path, destination)

    commands = npm_commands(text)
    missing = [command for command in commands if command not in package_scripts]
    for command in missing:
        missing_package_commands.append({"path": rel_path, "command": command})

    for item in forbidden_patterns:
        if item["pattern"] in text:
            forbidden_reference_hits.append(
                {
                    "path": rel_path,
                    "pattern": item["pattern"],
                    "category": item["category"],
                }
            )

    disposition = classify(script_name, missing)
    buckets[disposition].append(rel_path)
    items.append(
        {
            "path": rel_path,
            "snapshot_path": str(destination.relative_to(root)),
            "sha256": hashlib.sha256(data).hexdigest(),
            "line_count": text.count("\n") + (1 if text and not text.endswith("\n") else 0),
            "disposition": disposition,
            "referenced_npm_commands": commands,
            "missing_package_commands": missing,
        }
    )

snapshot_manifest = {
    "schema_version": "ao2.script-surface-snapshot.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "untracked_script_count": len(items),
    "scripts": items,
}
snapshot_manifest_path.write_text(
    json.dumps(snapshot_manifest, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)

report_lines = [
    "# AO2 Script Surface Audit",
    "",
    "This report preserves local RSI/Pulse shell scripts before any promotion decision.",
    "",
    f"- Untracked scripts: {len(items)}",
    f"- Promote candidates: {len(buckets['promote_candidates'])}",
    f"- Consolidate: {len(buckets['consolidate'])}",
    f"- Defer control-plane: {len(buckets['defer_control_plane'])}",
    f"- Local-only: {len(buckets['local_only'])}",
    f"- Missing package command references: {len(missing_package_commands)}",
    f"- Forbidden reference hits: {len(forbidden_reference_hits)}",
    "",
    "| Disposition | Script | Missing package commands |",
    "| --- | --- | --- |",
]
for item in items:
    missing = ", ".join(item["missing_package_commands"]) or "-"
    report_lines.append(f"| {item['disposition']} | `{item['path']}` | {missing} |")
report_path.write_text("\n".join(report_lines) + "\n", encoding="utf-8")

status = "failed" if forbidden_reference_hits else "passed"
provider_hits = [
    item for item in forbidden_reference_hits if item["category"] == "provider_api_key"
]
secret_path_hits = [
    item for item in forbidden_reference_hits if item["category"] == "secret_path"
]
publishing_hits = [
    item
    for item in forbidden_reference_hits
    if item["category"] == "publishing_side_effect"
]
payload = {
    "schema_version": "ao2.script-surface-audit.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "tracked_command_count": len(package_scripts),
    "untracked_script_count": len(items),
    "snapshot_manifest": str(snapshot_manifest_path),
    "classification_report": str(report_path),
    "buckets": buckets,
    "missing_package_commands": missing_package_commands,
    "forbidden_reference_hits": forbidden_reference_hits,
    "no_secret_paths": not secret_path_hits,
    "no_provider_api_keys": not provider_hits,
    "no_publishing_side_effect_references": not publishing_hits,
    "no_auto_promotion": True,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"snapshot_manifest={snapshot_manifest_path}")
print(f"classification_report={report_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
