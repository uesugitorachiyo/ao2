#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_SKILL_PACK_PARITY_ROOT:-$ROOT/target/operator-skill-pack-parity/latest}"
SUMMARY="$OUT_ROOT/summary.json"
REPORT="$OUT_ROOT/operator-skill-pack-parity.md"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$REPORT" <<'PY'
import hashlib
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
report_path = Path(sys.argv[4]).resolve()

package = json.loads((root / "package.json").read_text(encoding="utf-8"))
package_scripts = set(package.get("scripts", {}))
repo_skills_root = root / "skills"
claude_skills_root = root / ".claude" / "skills"

npm_ref_pattern = re.compile(r"\bnpm\s+run\s+([A-Za-z0-9:_./-]+)")


def skill_dirs(base):
    if not base.exists():
        return {}
    return {
        item.name: item
        for item in sorted(base.iterdir())
        if item.is_dir() and item.name.startswith("ao2-") and (item / "SKILL.md").exists()
    }


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def line_count(text):
    return text.count("\n") + (1 if text and not text.endswith("\n") else 0)


def parse_frontmatter(text):
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---\n", 4)
    if end == -1:
        return None
    fields = {}
    for raw_line in text[4:end].splitlines():
        if ":" not in raw_line:
            continue
        key, value = raw_line.split(":", 1)
        fields[key.strip()] = value.strip().strip('"').strip("'")
    return fields


def npm_commands(text):
    commands = set()
    for match in npm_ref_pattern.finditer(text):
        command = match.group(1).rstrip(";,).")
        if command and command != "--":
            commands.add(command)
    return sorted(commands)


repo_skill_dirs = skill_dirs(repo_skills_root)
claude_skill_dirs = skill_dirs(claude_skills_root)
repo_skill_names = set(repo_skill_dirs)
claude_skill_names = set(claude_skill_dirs)
all_skill_names = sorted(repo_skill_names | claude_skill_names)

missing_repo_skills = sorted(claude_skill_names - repo_skill_names)
missing_claude_skills = sorted(repo_skill_names - claude_skill_names)
mismatched_skill_files = []
frontmatter_failures = []
ascii_failures = []
missing_package_commands = []
skills = []

for skill_name in all_skill_names:
    repo_path = repo_skill_dirs.get(skill_name, repo_skills_root / skill_name) / "SKILL.md"
    claude_path = claude_skill_dirs.get(skill_name, claude_skills_root / skill_name) / "SKILL.md"
    repo_data = repo_path.read_bytes() if repo_path.exists() else b""
    claude_data = claude_path.read_bytes() if claude_path.exists() else b""

    if repo_path.exists() and claude_path.exists() and repo_data != claude_data:
        mismatched_skill_files.append(skill_name)

    reference_texts = []
    referenced_commands = set()
    for label, path, data in (
        ("repo", repo_path, repo_data),
        ("claude", claude_path, claude_data),
    ):
        if not path.exists():
            continue
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError as error:
            frontmatter_failures.append(
                {"skill": skill_name, "path": str(path.relative_to(root)), "reason": str(error)}
            )
            continue
        reference_texts.append(text)

        non_ascii_lines = [
            line_number
            for line_number, line in enumerate(text.splitlines(), start=1)
            if any(ord(character) > 127 for character in line)
        ]
        if non_ascii_lines:
            ascii_failures.append(
                {
                    "skill": skill_name,
                    "copy": label,
                    "path": str(path.relative_to(root)),
                    "lines": non_ascii_lines,
                }
            )

        frontmatter = parse_frontmatter(text)
        if frontmatter is None:
            frontmatter_failures.append(
                {
                    "skill": skill_name,
                    "copy": label,
                    "path": str(path.relative_to(root)),
                    "reason": "missing frontmatter block",
                }
            )
        else:
            if frontmatter.get("name") != skill_name:
                frontmatter_failures.append(
                    {
                        "skill": skill_name,
                        "copy": label,
                        "path": str(path.relative_to(root)),
                        "reason": "frontmatter name must match directory",
                        "actual": frontmatter.get("name"),
                    }
                )
            if not frontmatter.get("description"):
                frontmatter_failures.append(
                    {
                        "skill": skill_name,
                        "copy": label,
                        "path": str(path.relative_to(root)),
                        "reason": "frontmatter description is required",
                    }
                )

        for command in npm_commands(text):
            referenced_commands.add(command)
            if command not in package_scripts:
                missing_package_commands.append(
                    {
                        "skill": skill_name,
                        "copy": label,
                        "path": str(path.relative_to(root)),
                        "command": command,
                    }
                )

    skills.append(
        {
            "name": skill_name,
            "repo_path": str(repo_path.relative_to(root)),
            "claude_path": str(claude_path.relative_to(root)),
            "repo_sha256": sha256(repo_data) if repo_data else None,
            "claude_sha256": sha256(claude_data) if claude_data else None,
            "repo_line_count": line_count(reference_texts[0]) if reference_texts else 0,
            "referenced_npm_commands": sorted(referenced_commands),
        }
    )

failures = {
    "missing_repo_skills": missing_repo_skills,
    "missing_claude_skills": missing_claude_skills,
    "mismatched_skill_files": mismatched_skill_files,
    "frontmatter_failures": frontmatter_failures,
    "ascii_failures": ascii_failures,
    "missing_package_commands": missing_package_commands,
}
status = "passed" if not any(failures.values()) else "failed"

report_lines = [
    "# AO2 Operator Skill Pack Parity",
    "",
    "This report verifies AO2 operator skills stay synchronized across repo-native",
    "`skills/` and Claude-compatible `.claude/skills/` surfaces.",
    "",
    f"- Status: {status}",
    f"- Checked skills: {len(all_skill_names)}",
    f"- Missing repo skills: {len(missing_repo_skills)}",
    f"- Missing Claude skills: {len(missing_claude_skills)}",
    f"- Mismatched skill files: {len(mismatched_skill_files)}",
    f"- Frontmatter failures: {len(frontmatter_failures)}",
    f"- ASCII failures: {len(ascii_failures)}",
    f"- Missing package command references: {len(missing_package_commands)}",
    "",
    "| Skill | Repo SHA256 | Claude SHA256 | npm commands |",
    "| --- | --- | --- | --- |",
]
for item in skills:
    commands = ", ".join(f"`{command}`" for command in item["referenced_npm_commands"]) or "-"
    report_lines.append(
        f"| `{item['name']}` | `{item['repo_sha256'] or '-'}` | "
        f"`{item['claude_sha256'] or '-'}` | {commands} |"
    )
report_path.write_text("\n".join(report_lines) + "\n", encoding="utf-8")

payload = {
    "schema_version": "ao2.operator-skill-pack-parity.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "checked_skill_count": len(all_skill_names),
    "tracked_command_count": len(package_scripts),
    "report": str(report_path),
    "skills": skills,
    "failures": failures,
    "repo_and_claude_skill_sets_match": not missing_repo_skills and not missing_claude_skills,
    "skill_files_byte_for_byte_identical": not mismatched_skill_files,
    "frontmatter_valid": not frontmatter_failures,
    "ascii_only": not ascii_failures,
    "all_referenced_npm_commands_exist": not missing_package_commands,
    "no_auto_promotion": True,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "publishes": False,
        "mutates_repository": False,
    },
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"report={report_path}")
print(f"checked_skill_count={len(all_skill_names)}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
