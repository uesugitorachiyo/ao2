#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_INSTALL_UPDATE_ROOT:-$ROOT/target/release-install-update-fixture/latest}"
FIXTURE_DIR="${AO2_RELEASE_INSTALL_UPDATE_FIXTURE_DIR:-$OUT_ROOT/fixture-release}"
SUMMARY="$OUT_ROOT/summary.json"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT" "$FIXTURE_DIR"
OUT_ROOT="$(cd "$OUT_ROOT" && pwd)"
FIXTURE_DIR="$(cd "$FIXTURE_DIR" && pwd)"
SUMMARY="$OUT_ROOT/summary.json"

VERSION="$("$ROOT/scripts/current-version.sh")"
ARCHIVE="$FIXTURE_DIR/ao2-$VERSION-local-fixture.tar.gz"
EXTRACT="$OUT_ROOT/extract"
INSTALL_DIR="$OUT_ROOT/install"
UPDATE_DIR="$OUT_ROOT/update"
PAYLOAD_DIR="$OUT_ROOT/archive-payload"

mkdir -p "$PAYLOAD_DIR/bin" "$INSTALL_DIR" "$UPDATE_DIR"
cat >"$PAYLOAD_DIR/RELEASE-MANIFEST.json" <<JSON
{"schema_version":"ao2.release-manifest.v1","binary":"ao2","version":"$VERSION","target":"local-fixture"}
JSON
cat >"$PAYLOAD_DIR/install.sh" <<'SH'
#!/bin/sh
set -eu
install_dir="${AO2_INSTALL_DIR:-./bin}"
mkdir -p "$install_dir"
cp "$(dirname "$0")/bin/ao2" "$install_dir/ao2"
chmod 755 "$install_dir/ao2"
SH
chmod 755 "$PAYLOAD_DIR/install.sh"
cat >"$PAYLOAD_DIR/bin/ao2" <<SH
#!/bin/sh
if [ "\${1:-}" = "version" ]; then
  printf '{"schema_version":"ao2.version.v1","version":"%s","target":"local-fixture"}\n' "$VERSION"
else
  printf 'ao2 local fixture %s\n' "$VERSION"
fi
SH
chmod 755 "$PAYLOAD_DIR/bin/ao2"
tar -czf "$ARCHIVE" -C "$PAYLOAD_DIR" .

(cd "$FIXTURE_DIR" && shasum -a 256 "$(basename "$ARCHIVE")" > SHA256SUMS)
ARCHIVE_SHA="$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')"
cat >"$FIXTURE_DIR/provenance.json" <<JSON
{"schema_version":"ao2.release-provenance.v1","archive":"$(basename "$ARCHIVE")","sha256":"$ARCHIVE_SHA","signature":"local-fixture-signature"}
JSON
printf "local-fixture-signature %s\n" "$ARCHIVE_SHA" >"$FIXTURE_DIR/provenance.json.signature"

(cd "$FIXTURE_DIR" && shasum -a 256 -c SHA256SUMS >"$OUT_ROOT/checksum-verification.log")
mkdir -p "$EXTRACT"
tar -xzf "$ARCHIVE" -C "$EXTRACT"
AO2_INSTALL_DIR="$INSTALL_DIR" sh "$EXTRACT/install.sh" >"$OUT_ROOT/install-smoke.log"
"$INSTALL_DIR/ao2" version >"$OUT_ROOT/install-version.json"
cp "$INSTALL_DIR/ao2" "$UPDATE_DIR/ao2.previous"
AO2_INSTALL_DIR="$UPDATE_DIR" sh "$EXTRACT/install.sh" >"$OUT_ROOT/update-smoke.log"
"$UPDATE_DIR/ao2" version >"$OUT_ROOT/update-version.json"

python3 - "$SUMMARY" "$OUT_ROOT" "$FIXTURE_DIR" "$ARCHIVE" "$ARCHIVE_SHA" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

summary_path = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
fixture_dir = Path(sys.argv[3]).resolve()
archive = Path(sys.argv[4]).resolve()
archive_sha = sys.argv[5]
payload = {
    "schema_version": "ao2.release-install-update-fixture.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "artifact_root": str(out_root),
    "fixture_dir": str(fixture_dir),
    "archive": str(archive),
    "checksum_verification": str(out_root / "checksum-verification.log"),
    "install_smoke": str(out_root / "install-version.json"),
    "update_smoke": str(out_root / "update-version.json"),
    "SHA256SUMS": str(fixture_dir / "SHA256SUMS"),
    "provenance.json": str(fixture_dir / "provenance.json"),
    "signature": str(fixture_dir / "provenance.json.signature"),
    "sha256": archive_sha,
    "release:download-verify": "referenced for real release asset install/update verification",
    "publish_guards": {
        "refuses_publish_side_effects_by_default": True,
        "git_push_origin": "not executed",
        "gh_release_create": "not executed"
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("status=passed")
PY
