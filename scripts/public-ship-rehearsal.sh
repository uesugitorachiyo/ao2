#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PUBLIC_SHIP_REHEARSAL_ROOT:-$ROOT/target/public-ship-rehearsal/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
FIXTURE_DIR="${AO2_PUBLIC_SHIP_REHEARSAL_FIXTURE_DIR:-}"

rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

run_step() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  set +e
  "$@" >"$log" 2>&1
  local code=$?
  set -e
  printf "%s\n" "$code" >"$log.exit-code"
}

run_step public_release_train \
  env \
    AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT="$OUT_ROOT/public-release-train-drill" \
    AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR="$FIXTURE_DIR" \
    npm run release:train-drill

run_step release_readiness_static \
  env AO2_RELEASE_READINESS_ROOT="$OUT_ROOT/release-readiness-static" \
    npm run release:readiness:static

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$FIXTURE_DIR" <<'PY'
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
fixture_dir = sys.argv[4] or None
log_dir = out_root / "logs"
version = subprocess_version = None
current_version_script = root / "scripts" / "current-version.sh"
import subprocess
version = subprocess.check_output([str(current_version_script)], cwd=root, text=True).strip()
readme = (root / "README.md").read_text(encoding="utf-8", errors="replace")
install = (root / "docs" / "INSTALL.md").read_text(encoding="utf-8", errors="replace")
ready = (root / "docs" / "release" / "READY-TO-SHIP.md").read_text(encoding="utf-8", errors="replace")
manifest = json.loads((root / "public-export-manifest.json").read_text(encoding="utf-8"))
checks = []
for name in ["public_release_train", "release_readiness_static"]:
    code = int((log_dir / f"{name}.log.exit-code").read_text(encoding="utf-8").strip())
    checks.append({"name": name, "status": "passed" if code == 0 else "failed", "exit_code": code, "log": str(log_dir / f"{name}.log")})
version_docs_ok = bool(version) and all(text.strip() for text in [readme, install, ready]) and re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is not None
manifest_ok = manifest.get("trust_boundary", {}).get("provider_api_key_auth") == "forbidden"
checks.extend([
    {"name": "version_changelog_docs_consistency", "status": "passed" if version_docs_ok else "failed", "version": version},
    {"name": "public_export_manifest_consistency", "status": "passed" if manifest_ok else "failed"},
])
status = "passed" if all(check["status"] == "passed" for check in checks) else "failed"
payload = {
    "schema_version": "ao2.public-ship-rehearsal.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "artifact_root": str(out_root),
    "version": version,
    "release_artifact_fixture": fixture_dir,
    "checks": checks,
    "component_summaries": {
        "public_release_train": str(out_root / "public-release-train-drill" / "summary.json"),
        "release_readiness_static": str(out_root / "release-readiness-static" / "summary.json"),
    },
    "publish_guards": {"tag_push_publish_deploy": "not executed", "release_publish": "not executed"},
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={status}")
if status != "passed":
    raise SystemExit(1)
PY
