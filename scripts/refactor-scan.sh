#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

section() {
  printf '\n## %s\n' "$1"
}

section "Repository"
printf 'root=%s\n' "$ROOT"
printf 'branch=%s\n' "$(git branch --show-current 2>/dev/null || printf 'unknown')"
printf 'git_dir=%s\n' "$(git rev-parse --git-dir 2>/dev/null || printf 'unknown')"
printf 'git_common_dir=%s\n' "$(git rev-parse --git-common-dir 2>/dev/null || printf 'unknown')"

section "Workspace status"
if git diff --quiet --ignore-submodules -- && git diff --cached --quiet --ignore-submodules --; then
  printf 'tracked_changes=none\n'
else
  printf 'tracked_changes=present\n'
fi
git status --short

section "Detected stack"
for path in Cargo.toml package.json pnpm-workspace.yaml pytest.ini rust-toolchain.toml AGENTS.md skills .claude/skills .github/workflows docs scripts tests; do
  if [ -e "$path" ]; then
    printf 'present=%s\n' "$path"
  fi
done

section "Generated or local-only paths"
for path in target .ao2-local .ao2/runs .ao2/control-plane .ao2/memory dist dist-linux dist-linux-x86_64 dist-windows dist-provenance .release-signing node_modules; do
  if [ -e "$path" ]; then
    printf 'avoid=%s\n' "$path"
  fi
done

section "Validation commands"
if [ -f package.json ]; then
  python3 - <<'PY'
import json
from pathlib import Path

data = json.loads(Path("package.json").read_text(encoding="utf-8"))
scripts = data.get("scripts", {})
preferred = [
    "verify",
    "public:hardening",
    "ci:local",
    "gate:full",
    "control-plane:cross-repo-observer",
    "rsi:cross-repo-e2e",
    "scripts:tracking-decision-cleanup",
    "skills:operator-pack-parity",
]
for name in preferred:
    if name in scripts:
        print(f"npm_run={name}")
PY
fi
if [ -f Cargo.toml ]; then
  printf 'cargo=cargo fmt --all -- --check\n'
  printf 'cargo=cargo test --workspace\n'
  printf 'cargo=cargo clippy --workspace --all-targets -- -D warnings\n'
fi
if [ -d tests ]; then
  printf 'python=python3 -m pytest tests -q\n'
fi

section "Large or risky tracked surfaces"
git ls-files | awk '
  /^scripts\// { scripts += 1 }
  /^docs\// { docs += 1 }
  /^crates\// { crates += 1 }
  /^tests\// { tests += 1 }
  /^\.github\// { github += 1 }
  /^Cargo\.lock$/ { lockfiles += 1 }
  END {
    printf "tracked_scripts=%d\n", scripts
    printf "tracked_docs=%d\n", docs
    printf "tracked_crates=%d\n", crates
    printf "tracked_tests=%d\n", tests
    printf "tracked_github=%d\n", github
    printf "tracked_lockfiles=%d\n", lockfiles
  }
'

section "Recommended first gate"
printf 'docs_or_templates=bash scripts/refactor-check.sh docs\n'
printf 'shell_scripts=bash scripts/refactor-check.sh scripts\n'
printf 'rust_or_behavior=npm run verify\n'
