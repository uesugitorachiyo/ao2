#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
usage: bash scripts/refactor-check.sh <docs|scripts|rust|full>

docs    Check markdown/task docs and whitespace-sensitive diff issues.
scripts Check shell syntax for the refactor helper scripts and diff issues.
rust    Run Rust formatting check.
full    Run the repository's npm verify gate.
EOF
}

mode="${1:-}"
if [ -z "$mode" ] || [ "$mode" = "--help" ] || [ "$mode" = "-h" ]; then
  usage
  exit 0
fi

run() {
  printf '\n## %s\n' "$*"
  "$@"
}

check_text_files() {
  python3 - "$@" <<'PY'
import sys
from pathlib import Path

failed = False
for raw in sys.argv[1:]:
    path = Path(raw)
    if not path.is_file():
        continue
    data = path.read_bytes()
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as error:
        print(f"{path}: utf8_error: {error}")
        failed = True
        continue
    if data and not data.endswith(b"\n"):
        print(f"{path}: missing_final_newline")
        failed = True
    for index, line in enumerate(text.splitlines(), start=1):
        if line.rstrip(" \t") != line:
            print(f"{path}:{index}: trailing_whitespace")
            failed = True
if failed:
    raise SystemExit(1)
print("text_check=passed")
PY
}

case "$mode" in
  docs)
    run check_text_files \
      docs/refactor-cleanup/REFACTOR_AGENT_HANDOFF_TEMPLATE.md \
      docs/refactor-cleanup/REFACTOR_CLEANUP_WORKFLOW.md \
      docs/refactor-cleanup/REFACTOR_TASK_TEMPLATE.md \
      docs/refactor-cleanup/cleanup-agent-loop.md \
      scripts/refactor-scan.sh \
      scripts/refactor-check.sh
    run git status --short -- docs/refactor-cleanup scripts/refactor-scan.sh scripts/refactor-check.sh
    ;;
  scripts)
    run bash -n scripts/refactor-scan.sh
    run bash -n scripts/refactor-check.sh
    run check_text_files scripts/refactor-scan.sh scripts/refactor-check.sh
    run git status --short -- scripts/refactor-scan.sh scripts/refactor-check.sh
    ;;
  rust)
    run cargo fmt --all -- --check
    ;;
  full)
    run npm run verify
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
