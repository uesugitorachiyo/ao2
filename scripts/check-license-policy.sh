#!/usr/bin/env bash
set -euo pipefail

scan_archives=false
if [[ "${1:-}" == "--scan-archives" ]]; then
  scan_archives=true
elif [[ "${1:-}" != "" ]]; then
  printf 'usage: %s [--scan-archives]\n' "$0" >&2
  exit 2
fi

fail() {
  printf 'license policy check failed: %s\n' "$1" >&2
  exit 1
}

[[ -f LICENSE ]] || fail "missing root LICENSE"
[[ -f NOTICE ]] || fail "missing root NOTICE"

for file in LICENSE-APACHE LICENSE-MIT COPYING COPYING.txt; do
  [[ ! -e "$file" ]] || fail "unexpected extra license file: $file"
done

grep -Eq '^[[:space:]]*Apache License$' LICENSE || fail "LICENSE is not canonical Apache-2.0 text"
grep -Eq '^[[:space:]]*Version 2\.0, January 2004$' LICENSE || fail "LICENSE is not Apache-2.0"
grep -qx '   END OF TERMS AND CONDITIONS' LICENSE || fail "LICENSE is missing canonical Apache appendix boundary"
grep -qx '   APPENDIX: How to apply the Apache License to your work.' LICENSE || fail "LICENSE is missing canonical Apache appendix"

if [[ -f Cargo.toml ]]; then
  grep -Eq '^license = "Apache-2\.0"$' Cargo.toml || fail "Cargo.toml must declare license = \"Apache-2.0\""
fi

if [[ -f package.json ]]; then
  node -e 'const p=require("./package.json"); if (p.license !== "Apache-2.0") { throw new Error("package.json must declare Apache-2.0"); }'
fi

if [[ -f pyproject.toml ]]; then
  python3 - <<'PY'
from pathlib import Path
text = Path("pyproject.toml").read_text(encoding="utf-8")
if 'license = "Apache-2.0"' not in text and 'license = { text = "Apache-2.0" }' not in text:
    raise SystemExit("pyproject.toml must declare Apache-2.0")
PY
fi

if [[ -f plugin.json ]]; then
  python3 - <<'PY'
import json
from pathlib import Path
payload = json.loads(Path("plugin.json").read_text(encoding="utf-8"))
if payload.get("license") != "Apache-2.0":
    raise SystemExit("plugin.json must declare Apache-2.0")
PY
fi

if [[ "$scan_archives" == "true" ]]; then
  while IFS= read -r -d '' archive; do
    case "$archive" in
      *.tar.gz|*.tgz)
        entries="$(tar -tzf "$archive")"
        ;;
      *.zip|*.whl)
        entries="$(python3 - "$archive" <<'PY'
import sys
from zipfile import ZipFile
with ZipFile(sys.argv[1]) as zf:
    print("\n".join(zf.namelist()))
PY
)"
        ;;
      *)
        continue
        ;;
    esac
    printf '%s\n' "$entries" | grep -Eq '(^|/)LICENSE$' || fail "$archive missing LICENSE"
    printf '%s\n' "$entries" | grep -Eq '(^|/)NOTICE$' || fail "$archive missing NOTICE"
  done < <(find dist dist-linux dist-linux-x86_64 dist-windows release -type f \( -name '*.tar.gz' -o -name '*.tgz' -o -name '*.zip' -o -name '*.whl' \) -print0 2>/dev/null)
fi

printf 'license policy check passed\n'
