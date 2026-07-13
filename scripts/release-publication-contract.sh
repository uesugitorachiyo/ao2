#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_VERSION="${AO2_VERSION:-$("$ROOT/scripts/current-version.sh")}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$AO2_VERSION}"
AO2_RELEASE_CHANNEL="${AO2_RELEASE_CHANNEL:-}"
AO2_RELEASE_TITLE="${AO2_RELEASE_TITLE:-}"
AO2_RELEASE_NOTES_FILE="${AO2_RELEASE_NOTES_FILE:-}"
AO2_RELEASE_PRIVATE_KEY="${AO2_RELEASE_PRIVATE_KEY:-$ROOT/.release-signing/ao2-release-signing-key.pem}"
AO2_RELEASE_PUBLICATION_DIR="${AO2_RELEASE_PUBLICATION_DIR:-$ROOT/target/release-publication/$AO2_RELEASE_TAG}"
AO2_RELEASE_CONTRACT_REQUIRE_ASSETS="${AO2_RELEASE_CONTRACT_REQUIRE_ASSETS:-1}"
AO2_RELEASE_CONTRACT_MODE="build-and-publish"

case "${1:-}" in
  '') ;;
  --promote-approved-assets) AO2_RELEASE_CONTRACT_MODE="promote-approved-assets" ;;
  *)
    echo "release publication contract failed: unsupported argument: $1" >&2
    exit 1
    ;;
esac
[ "$#" -le 1 ] || {
  echo "release publication contract failed: too many arguments" >&2
  exit 1
}

fail() {
  echo "release publication contract failed: $*" >&2
  exit 1
}

[ "$AO2_RELEASE_TAG" = "v$AO2_VERSION" ] || fail "release tag must equal v$AO2_VERSION"
[ -n "$AO2_RELEASE_NOTES_FILE" ] || fail "AO2_RELEASE_NOTES_FILE must be supplied explicitly"
[ -s "$AO2_RELEASE_NOTES_FILE" ] || fail "release notes file is missing or empty: $AO2_RELEASE_NOTES_FILE"
[ -n "$AO2_RELEASE_TITLE" ] || fail "AO2_RELEASE_TITLE must be supplied explicitly"
case "$AO2_RELEASE_CONTRACT_MODE" in
  build-and-publish)
    [ -f "$AO2_RELEASE_PRIVATE_KEY" ] || fail "release signing material is missing: $AO2_RELEASE_PRIVATE_KEY"
    ;;
  promote-approved-assets) ;;
  *) fail "unsupported release publication contract mode: $AO2_RELEASE_CONTRACT_MODE" ;;
esac

if printf '%s\n' "$AO2_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*$'; then
    [ "$AO2_RELEASE_CHANNEL" = "prerelease" ] || fail "prerelease version requires AO2_RELEASE_CHANNEL=prerelease"
    case "$(printf '%s' "$AO2_RELEASE_TITLE" | tr '[:upper:]' '[:lower:]')" in
      *external*beta*) ;;
      *) fail "prerelease title must identify the release as an external beta" ;;
    esac
    grep -Eiq 'external beta' "$AO2_RELEASE_NOTES_FILE" || fail "prerelease notes must identify the release as an external beta"
elif printf '%s\n' "$AO2_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    [ "$AO2_RELEASE_CHANNEL" = "stable" ] || fail "stable version requires AO2_RELEASE_CHANNEL=stable"
else
  fail "AO2_VERSION is not a supported semantic version: $AO2_VERSION"
fi

for pilot in \
  AO2_RELEASE_CODEX_PILOT_ACCEPTANCE \
  AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE \
  AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE; do
  [ "${!pilot:-0}" = "0" ] || fail "$pilot must remain disabled during publication"
done

if [ "$AO2_RELEASE_CONTRACT_REQUIRE_ASSETS" = "1" ]; then
  [ -d "$AO2_RELEASE_PUBLICATION_DIR" ] || fail "publication asset directory is missing: $AO2_RELEASE_PUBLICATION_DIR"
  for target in macos-aarch64 linux-aarch64 linux-x86_64 windows-x86_64; do
    for suffix in tar.gz tar.gz.sha256 tar.gz.sig sbom.cdx.json; do
      asset="$AO2_RELEASE_PUBLICATION_DIR/ao2-$AO2_VERSION-$target.$suffix"
      [ -s "$asset" ] || fail "required release asset is missing: $asset"
    done
  done
  for name in \
    SHA256SUMS \
    ao2-release-provenance.json \
    ao2-release-provenance.json.sig \
    ao2-release-signing-public.pem \
    ao2-release-artifact-closure-index.json \
    ao2-release-readiness-summary.json \
    ao2-release-train-control-plane-bridge-summary.json; do
    [ -s "$AO2_RELEASE_PUBLICATION_DIR/$name" ] || fail "required release asset is missing: $AO2_RELEASE_PUBLICATION_DIR/$name"
  done
  (cd "$AO2_RELEASE_PUBLICATION_DIR" && shasum -a 256 -c SHA256SUMS >/dev/null)
fi

printf 'release_publication_contract=passed\n'
printf 'release_channel=%s\n' "$AO2_RELEASE_CHANNEL"
printf 'release_tag=%s\n' "$AO2_RELEASE_TAG"
