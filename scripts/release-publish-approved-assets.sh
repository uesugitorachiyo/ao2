#!/usr/bin/env bash
set -euo pipefail

# Promote one immutable, operator-approved AO2 asset set. This command only
# verifies and publishes files already present in the supplied directory.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  printf 'approved asset publication failed: %s\n' "$*" >&2
  exit 1
}

require_value() {
  local name="$1"
  local value="$2"
  [ -n "$value" ] || fail "$name must be supplied explicitly"
}

sha256_file() {
  local path="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  else
    fail "shasum or sha256sum is required"
  fi
}

AO2_VERSION="${AO2_VERSION:-}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-}"
AO2_RELEASE_TARGET_COMMIT="${AO2_RELEASE_TARGET_COMMIT:-}"
AO2_RELEASE_CHANNEL="${AO2_RELEASE_CHANNEL:-}"
AO2_RELEASE_TITLE="${AO2_RELEASE_TITLE:-}"
AO2_RELEASE_NOTES_FILE="${AO2_RELEASE_NOTES_FILE:-}"
AO2_RELEASE_NOTES_SHA256="${AO2_RELEASE_NOTES_SHA256:-}"
AO2_RELEASE_PUBLICATION_DIR="${AO2_RELEASE_PUBLICATION_DIR:-}"
AO2_RELEASE_PUBLICATION_LIST="${AO2_RELEASE_PUBLICATION_LIST:-}"
AO2_RELEASE_EXPECTED_ASSET_MANIFEST="${AO2_RELEASE_EXPECTED_ASSET_MANIFEST:-}"
AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256="${AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256:-}"
AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG="${AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG:-}"
AO2_RELEASE_PUBLISH_APPROVED_MODE="${AO2_RELEASE_PUBLISH_APPROVED_MODE:-}"
AO2_RELEASE_PUBLISH_APPROVED_CONFIRM="${AO2_RELEASE_PUBLISH_APPROVED_CONFIRM:-}"

for required in \
  AO2_VERSION \
  AO2_RELEASE_REPO \
  AO2_RELEASE_TAG \
  AO2_RELEASE_TARGET_COMMIT \
  AO2_RELEASE_CHANNEL \
  AO2_RELEASE_TITLE \
  AO2_RELEASE_NOTES_FILE \
  AO2_RELEASE_NOTES_SHA256 \
  AO2_RELEASE_PUBLICATION_DIR \
  AO2_RELEASE_PUBLICATION_LIST \
  AO2_RELEASE_EXPECTED_ASSET_MANIFEST \
  AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256 \
  AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG \
  AO2_RELEASE_PUBLISH_APPROVED_MODE \
  AO2_RELEASE_PUBLISH_APPROVED_CONFIRM; do
  require_value "$required" "${!required}"
done

case "$AO2_RELEASE_PUBLISH_APPROVED_MODE" in
  dry-run|live) ;;
  *) fail "AO2_RELEASE_PUBLISH_APPROVED_MODE must be dry-run or live" ;;
esac

[ "$AO2_RELEASE_TAG" = "v$AO2_VERSION" ] || fail "release tag must equal v$AO2_VERSION"
case "$AO2_RELEASE_REPO" in
  */*) ;;
  *) fail "release repository must use owner/name format" ;;
esac
case "$AO2_VERSION" in
  *-*)
    [ "$AO2_RELEASE_CHANNEL" = "prerelease" ] || \
      fail "prerelease version requires AO2_RELEASE_CHANNEL=prerelease"
    ;;
  *) fail "approved asset promotion is restricted to an AO2 prerelease" ;;
esac
case "$(printf '%s' "$AO2_RELEASE_TITLE" | tr '[:upper:]' '[:lower:]')" in
  *external*beta*) ;;
  *) fail "prerelease title must identify the release as an external beta" ;;
esac

case "$AO2_RELEASE_TARGET_COMMIT" in
  *[!0-9a-f]*|'') fail "runtime target commit must be lowercase hexadecimal" ;;
esac
[ "${#AO2_RELEASE_TARGET_COMMIT}" -eq 40 ] || fail "runtime target commit must contain 40 hexadecimal characters"
case "$AO2_RELEASE_NOTES_SHA256" in
  *[!0-9a-f]*|'') fail "release notes SHA-256 must be lowercase hexadecimal" ;;
esac
[ "${#AO2_RELEASE_NOTES_SHA256}" -eq 64 ] || fail "release notes SHA-256 must contain 64 hexadecimal characters"
case "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" in
  *[!0-9a-f]*|'') fail "approved manifest SHA-256 must be lowercase hexadecimal" ;;
esac
[ "${#AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256}" -eq 64 ] || fail "approved manifest SHA-256 must contain 64 hexadecimal characters"

expected_confirmation="publish-approved-$AO2_RELEASE_TAG-$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"
[ "$AO2_RELEASE_PUBLISH_APPROVED_CONFIRM" = "$expected_confirmation" ] || \
  fail "exact approved promotion confirmation is required: $expected_confirmation"

[ -f "$AO2_RELEASE_NOTES_FILE" ] && [ ! -L "$AO2_RELEASE_NOTES_FILE" ] || \
  fail "release notes must be a regular non-symlink file"
observed_notes_sha256="$(sha256_file "$AO2_RELEASE_NOTES_FILE")"
[ "$observed_notes_sha256" = "$AO2_RELEASE_NOTES_SHA256" ] || \
  fail "release notes SHA-256 mismatch: expected $AO2_RELEASE_NOTES_SHA256, observed $observed_notes_sha256"
grep -Eiq 'external beta' "$AO2_RELEASE_NOTES_FILE" || fail "release notes must identify the release as an external beta"
grep -Fq 'ao2 install rollback' "$AO2_RELEASE_NOTES_FILE" || fail "release notes must include the approved rollback command"

git cat-file -e "$AO2_RELEASE_TARGET_COMMIT^{commit}" 2>/dev/null || \
  fail "runtime target commit does not exist: $AO2_RELEASE_TARGET_COMMIT"
origin_url="$(git remote get-url origin)"
case "$origin_url" in
  "https://github.com/$AO2_RELEASE_REPO"|"https://github.com/$AO2_RELEASE_REPO.git"|\
  "git@github.com:$AO2_RELEASE_REPO"|"git@github.com:$AO2_RELEASE_REPO.git"|\
  "ssh://git@github.com/$AO2_RELEASE_REPO"|"ssh://git@github.com/$AO2_RELEASE_REPO.git") ;;
  *) fail "origin remote does not match release repository" ;;
esac
[ -z "$(git status --porcelain)" ] || fail "refusing approved asset publication from a dirty worktree"
publisher_head="$(git rev-parse HEAD)"
publisher_origin_main="$(git rev-parse origin/main)"
[ "$publisher_head" = "$publisher_origin_main" ] || \
  fail "publisher implementation HEAD does not match origin/main"

if git rev-parse -q --verify "refs/tags/$AO2_RELEASE_TAG" >/dev/null 2>&1 \
  || git ls-remote --exit-code --tags origin "refs/tags/$AO2_RELEASE_TAG" >/dev/null 2>&1; then
  fail "refusing to reuse existing release tag: $AO2_RELEASE_TAG"
fi
if gh release view "$AO2_RELEASE_TAG" --repo "$AO2_RELEASE_REPO" >/dev/null 2>&1; then
  fail "refusing to overwrite existing release: $AO2_RELEASE_TAG"
fi
observed_latest_stable="$(gh api "repos/$AO2_RELEASE_REPO/releases/latest" --jq .tag_name)"
[ "$observed_latest_stable" = "$AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG" ] || \
  fail "latest stable release mismatch: expected $AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG, observed $observed_latest_stable"

verify_approved_assets() {
  python3 "$ROOT/scripts/release-verify-approved-assets.py" \
    --manifest "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST" \
    --manifest-sha256 "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" \
    --publication-dir "$AO2_RELEASE_PUBLICATION_DIR" \
    --publication-list "$AO2_RELEASE_PUBLICATION_LIST"
}

verify_approved_assets

AO2_RELEASE_CHANNEL="$AO2_RELEASE_CHANNEL" \
AO2_RELEASE_TITLE="$AO2_RELEASE_TITLE" \
AO2_RELEASE_NOTES_FILE="$AO2_RELEASE_NOTES_FILE" \
AO2_RELEASE_PUBLICATION_DIR="$AO2_RELEASE_PUBLICATION_DIR" \
  "$ROOT/scripts/release-publication-contract.sh" --promote-approved-assets

public_key="$AO2_RELEASE_PUBLICATION_DIR/ao2-release-signing-public.pem"
for target in macos-aarch64 linux-aarch64 linux-x86_64 windows-x86_64; do
  archive="$AO2_RELEASE_PUBLICATION_DIR/ao2-$AO2_VERSION-$target.tar.gz"
  signature="$archive.sig"
  openssl dgst -sha256 -verify "$public_key" -signature "$signature" "$archive" >/dev/null || \
    fail "archive signature verification failed: $(basename "$archive")"
done
openssl dgst -sha256 -verify "$public_key" \
  -signature "$AO2_RELEASE_PUBLICATION_DIR/ao2-release-provenance.json.sig" \
  "$AO2_RELEASE_PUBLICATION_DIR/ao2-release-provenance.json" >/dev/null || \
  fail "provenance signature verification failed"
printf 'release_approved_artifact_signatures=passed\n'

python3 - "$AO2_RELEASE_PUBLICATION_DIR" "$AO2_VERSION" "$AO2_RELEASE_TAG" "$AO2_RELEASE_TARGET_COMMIT" <<'PY'
from __future__ import annotations

import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile


publication = Path(sys.argv[1])
version, tag, commit = sys.argv[2:5]
targets = ("macos-aarch64", "linux-aarch64", "linux-x86_64", "windows-x86_64")
sha_re = re.compile(r"[0-9a-f]{64}")


def fail(message: str) -> None:
    raise SystemExit(f"approved artifact content verification failed: {message}")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json_bytes(data: bytes, label: str) -> object:
    try:
        return json.loads(data)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid JSON in {label}: {error}")


def load_json_file(path: Path) -> object:
    try:
        return json.loads(path.read_text())
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid JSON in {path.name}: {error}")


provenance_path = publication / "ao2-release-provenance.json"
provenance = load_json_file(provenance_path)
if not isinstance(provenance, dict):
    fail("provenance root must be an object")
expected_provenance = {
    "schema_version": "ao2.release-provenance.v1",
    "version": version,
    "release_tag": tag,
    "git_commit": commit,
}
for key, expected in expected_provenance.items():
    if provenance.get(key) != expected:
        fail(f"provenance {key} mismatch")
archive_records = provenance.get("archives")
if not isinstance(archive_records, list):
    fail("provenance archives must be an array")
records = {}
for record in archive_records:
    if not isinstance(record, dict) or not isinstance(record.get("name"), str):
        fail("provenance archive record is malformed")
    name = record["name"]
    if name in records:
        fail(f"duplicate provenance archive: {name}")
    records[name] = record

expected_archives = {f"ao2-{version}-{target}.tar.gz" for target in targets}
if set(records) != expected_archives:
    fail("provenance archive set mismatch")

required_common = {
    "BUILD-PROVENANCE.json",
    "LICENSE",
    "NOTICE",
    "README.txt",
    "RELEASE-MANIFEST.json",
    "RELEASE-VERIFICATION.json",
    "SBOM.cdx.json",
    "SHA256SUMS",
    "UNINSTALL.txt",
    "VERSION",
    "Verify-Release.ps1",
    "install.ps1",
    "install.sh",
    "verify-release.sh",
}

for target in targets:
    archive_name = f"ao2-{version}-{target}.tar.gz"
    archive_path = publication / archive_name
    archive_digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    record = records[archive_name]
    if record.get("sha256") != archive_digest:
        fail(f"provenance digest mismatch: {archive_name}")
    if record.get("checksum") != f"{archive_name}.sha256":
        fail(f"provenance checksum sidecar mismatch: {archive_name}")
    if record.get("signature") != f"{archive_name}.sig":
        fail(f"provenance signature sidecar mismatch: {archive_name}")
    sidecar = (publication / f"{archive_name}.sha256").read_text()
    if sidecar != f"{archive_digest}  {archive_name}\n":
        fail(f"archive checksum sidecar mismatch: {archive_name}")

    try:
        bundle = tarfile.open(archive_path, "r:gz")
    except (OSError, tarfile.TarError) as error:
        fail(f"cannot open archive {archive_name}: {error}")
    with bundle:
        members = bundle.getmembers()
        names = []
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or "\\" in member.name:
                fail(f"unsafe archive path in {archive_name}: {member.name}")
            if member.issym() or member.islnk():
                fail(f"archive link is not allowed in {archive_name}: {member.name}")
            if member.isfile():
                names.append(member.name)
        if len(names) != len(set(names)):
            fail(f"duplicate archive member in {archive_name}")
        archive_names = set(names)
        binary = "ao2.exe" if target == "windows-x86_64" else "ao2"
        required = required_common | {f"bin/{binary}"}
        if archive_names != required:
            fail(f"archive content set mismatch: {archive_name}")

        def read(name: str) -> bytes:
            handle = bundle.extractfile(name)
            if handle is None:
                fail(f"missing archive entry {name} in {archive_name}")
            return handle.read()

        if read("VERSION").decode().strip() != version:
            fail(f"archive version mismatch: {archive_name}")
        build = load_json_bytes(read("BUILD-PROVENANCE.json"), f"{archive_name}:BUILD-PROVENANCE.json")
        if not isinstance(build, dict):
            fail(f"build provenance root mismatch: {archive_name}")
        for key, expected in {
            "schema_version": "ao2.build-provenance.v1",
            "build_profile": "release",
            "git_commit": commit,
            "target": target,
            "version": version,
        }.items():
            if build.get(key) != expected:
                fail(f"archive build provenance {key} mismatch: {archive_name}")
        manifest = load_json_bytes(read("RELEASE-MANIFEST.json"), f"{archive_name}:RELEASE-MANIFEST.json")
        if not isinstance(manifest, dict):
            fail(f"release manifest root mismatch: {archive_name}")
        for key, expected in {
            "schema_version": "ao2.release-manifest.v1",
            "version": version,
            "target": target,
            "binary": binary,
            "binary_path": f"bin/{binary}",
            "uninstall": "UNINSTALL.txt",
        }.items():
            if manifest.get(key) != expected:
                fail(f"archive release manifest {key} mismatch: {archive_name}")
        if set(manifest.get("files", [])) != archive_names:
            fail(f"release manifest file set mismatch: {archive_name}")
        if set(manifest.get("legal_files", [])) != {"LICENSE", "NOTICE"}:
            fail(f"release manifest legal files mismatch: {archive_name}")
        if set(manifest.get("installers", [])) != {"install.sh", "install.ps1"}:
            fail(f"release manifest installers mismatch: {archive_name}")
        if set(manifest.get("verifiers", [])) != {"verify-release.sh", "Verify-Release.ps1"}:
            fail(f"release manifest verifiers mismatch: {archive_name}")

        verification = load_json_bytes(read("RELEASE-VERIFICATION.json"), f"{archive_name}:RELEASE-VERIFICATION.json")
        if not isinstance(verification, dict):
            fail(f"release verification root mismatch: {archive_name}")
        for key, expected in {
            "schema_version": "ao2.release-archive-offline-verification.v1",
            "status": "packaged",
            "version": version,
            "target": target,
            "provider_api_keys_required": False,
            "control_plane_approves_release": False,
            "mutates_ao_artifacts": False,
        }.items():
            if verification.get(key) != expected:
                fail(f"release verification {key} mismatch: {archive_name}")

        sbom_bytes = read("SBOM.cdx.json")
        staged_sbom = publication / f"ao2-{version}-{target}.sbom.cdx.json"
        if sbom_bytes != staged_sbom.read_bytes():
            fail(f"staged SBOM differs from archive: {archive_name}")
        sbom = load_json_bytes(sbom_bytes, f"{archive_name}:SBOM.cdx.json")
        if not isinstance(sbom, dict) or sbom.get("bomFormat") != "CycloneDX" or sbom.get("specVersion") != "1.5":
            fail(f"CycloneDX SBOM metadata mismatch: {archive_name}")
        component = sbom.get("metadata", {}).get("component", {})
        if component.get("name") != "ao2" or component.get("version") != version:
            fail(f"CycloneDX component mismatch: {archive_name}")

        checksum_text = read("SHA256SUMS").decode("utf-8")
        checksum_entries = {}
        for line_number, line in enumerate(checksum_text.splitlines(), 1):
            match = re.fullmatch(r"([0-9a-f]{64})  ([^/].*)", line)
            if match is None:
                fail(f"malformed archive checksum line {line_number}: {archive_name}")
            expected_digest, name = match.groups()
            path = PurePosixPath(name)
            if path.is_absolute() or ".." in path.parts or "\\" in name or name in checksum_entries:
                fail(f"unsafe or duplicate archive checksum path: {archive_name}")
            checksum_entries[name] = expected_digest
        if set(checksum_entries) != archive_names - {"SHA256SUMS"}:
            fail(f"archive checksum coverage mismatch: {archive_name}")
        for name, expected_digest in checksum_entries.items():
            if digest(read(name)) != expected_digest:
                fail(f"archive checksum mismatch for {name}: {archive_name}")

        for name in ("LICENSE", "NOTICE", "install.sh", "install.ps1", "verify-release.sh", "Verify-Release.ps1"):
            if not read(name).strip():
                fail(f"empty required archive content {name}: {archive_name}")
        uninstall = read("UNINSTALL.txt").decode("utf-8")
        if "rollback" not in uninstall or "install-verification" not in uninstall:
            fail(f"uninstall and rollback material mismatch: {archive_name}")

for name in (
    "ao2-release-artifact-closure-index.json",
    "ao2-release-readiness-summary.json",
    "ao2-release-train-control-plane-bridge-summary.json",
):
    if not isinstance(load_json_file(publication / name), dict):
        fail(f"release closure metadata root mismatch: {name}")

print("release_approved_artifact_contents=passed")
print("release_approved_provenance_binding=passed")
print("release_approved_sboms=passed")
PY

verify_approved_assets

if [ "$AO2_RELEASE_PUBLISH_APPROVED_MODE" = "dry-run" ]; then
  printf 'release_approval_bound=true\n'
  printf 'release_approved_asset_manifest_sha256=%s\n' "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"
  printf 'release_approved_notes_sha256=%s\n' "$observed_notes_sha256"
  printf 'release_publish_approved_assets_dry_run=passed\n'
  printf 'release_publish_approved_assets_mutations=not_executed\n'
  exit 0
fi

git tag -a "$AO2_RELEASE_TAG" "$AO2_RELEASE_TARGET_COMMIT" -m "$AO2_RELEASE_TITLE"
git push origin "$AO2_RELEASE_TAG"

verify_approved_assets

assets=()
while IFS= read -r asset; do
  [ -n "$asset" ] || continue
  assets+=("$AO2_RELEASE_PUBLICATION_DIR/$asset")
done < "$AO2_RELEASE_PUBLICATION_LIST"
[ "${#assets[@]}" -eq 23 ] || fail "approved publication list must contain exactly 23 assets"

release_url="$(gh release create "$AO2_RELEASE_TAG" "${assets[@]}" \
  --repo "$AO2_RELEASE_REPO" \
  --verify-tag \
  --title "$AO2_RELEASE_TITLE" \
  --notes-file "$AO2_RELEASE_NOTES_FILE" \
  --prerelease \
  --latest=false)"

printf 'release_approval_bound=true\n'
printf 'release_approved_asset_manifest_sha256=%s\n' "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"
printf 'release_approved_notes_sha256=%s\n' "$observed_notes_sha256"
printf 'release_publish_approved_assets_url=%s\n' "$release_url"
printf 'release_publish_approved_assets=passed\n'
