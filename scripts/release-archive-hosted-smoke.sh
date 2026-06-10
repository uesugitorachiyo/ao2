#!/bin/sh
set -eu

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64|Linux-amd64)
    target_label="${AO2_RELEASE_HOSTED_TARGET_LABEL:-linux-x86_64}"
    binary="${AO2_RELEASE_HOSTED_BINARY:-target/release/ao2}"
    ;;
  Darwin-arm64|Darwin-aarch64)
    target_label="${AO2_RELEASE_HOSTED_TARGET_LABEL:-macos-aarch64}"
    binary="${AO2_RELEASE_HOSTED_BINARY:-target/release/ao2}"
    ;;
  *)
    echo "unsupported hosted release archive smoke host: $(uname -s)-$(uname -m)" >&2
    exit 2
    ;;
esac

version="${AO2_RELEASE_HOSTED_VERSION:-$(scripts/current-version.sh)}"
root="${AO2_RELEASE_HOSTED_SMOKE_ROOT:-target/release-archive-hosted-smoke/$target_label}"
summary_json="${AO2_RELEASE_HOSTED_SMOKE_JSON:-$root/summary.json}"
dist="$root/dist"
extract="$root/extract"
install_dir="$root/bin"
archive="$dist/ao2-$version-$target_label.tar.gz"

rm -rf "$root"
mkdir -p "$dist" "$extract" "$install_dir"

test -f "$binary"
cargo run -p ao2-cli -- release package \
  --out-dir "$dist" \
  --version "$version" \
  --target-label "$target_label" \
  --binary "$binary" >/tmp/ao2-release-hosted-package.json

test -f "$archive"
tar -xzf "$archive" -C "$extract"
test -f "$extract/RELEASE-MANIFEST.json"
grep -q '"schema_version": "ao2.release-manifest.v1"' "$extract/RELEASE-MANIFEST.json"
grep -q "\"target\": \"$target_label\"" "$extract/RELEASE-MANIFEST.json"
grep -q '"binary": "ao2"' "$extract/RELEASE-MANIFEST.json"

AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh" >/tmp/ao2-release-hosted-install.out
installed="$install_dir/ao2"
install_verification_evidence="$install_dir/ao2.install-verification.json"
test -f "$installed"
test -f "$install_verification_evidence"
grep -q '"schema_version": "ao2.install-verification-evidence.v1"' "$install_verification_evidence"
grep -q '"status": "verified"' "$install_verification_evidence"
grep -q '"provider_api_keys_required": false' "$install_verification_evidence"
grep -q '"control_plane_approves_release": false' "$install_verification_evidence"
grep -q '"mutates_ao_artifacts": false' "$install_verification_evidence"
grep -q '"release_acceptance_owner": "factory-v3 evaluator-closer"' "$install_verification_evidence"

"$installed" --help >/dev/null
"$installed" version --json >"$root/version.json"
"$installed" adapter doctor --provider scripted >"$root/scripted-doctor.txt"
"$installed" provider matrix --json >"$root/provider-matrix.json"

mkdir -p "$(dirname -- "$summary_json")"
export archive install_verification_evidence installed root summary_json target_label version
python3 - <<'PY'
import json
import os
from pathlib import Path

summary = {
    "schema_version": "ao2.release-archive-hosted-smoke.v1",
    "status": "passed",
    "target": os.environ["target_label"],
    "version": os.environ["version"],
    "archive": os.environ["archive"],
    "installed_binary": os.environ["installed"],
    "install_verification_evidence": os.environ["install_verification_evidence"],
    "install_verification_schema": "ao2.install-verification-evidence.v1",
    "provider_api_keys_required": False,
    "control_plane_approves_release": False,
    "mutates_ao_artifacts": False,
    "release_acceptance_owner": "factory-v3 evaluator-closer",
}
Path(os.environ["summary_json"]).write_text(
    json.dumps(summary, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY

printf "release_archive_hosted_smoke=passed\n"
printf "target=%s\n" "$target_label"
printf "summary=%s\n" "$summary_json"
printf "install_verification_evidence=%s\n" "$install_verification_evidence"
