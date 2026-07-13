#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_VERSION="${AO2_VERSION:-$("$ROOT/scripts/current-version.sh")}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$AO2_VERSION}"
AO2_RELEASE_PUBLICATION_DIR="${AO2_RELEASE_PUBLICATION_DIR:-$ROOT/target/release-publication/$AO2_RELEASE_TAG}"
AO2_RELEASE_PUBLICATION_LIST="${AO2_RELEASE_PUBLICATION_LIST:-$ROOT/target/release-publication/$AO2_RELEASE_TAG.assets.txt}"
AO2_RELEASE_ARTIFACT_CLOSURE_INDEX="${AO2_RELEASE_ARTIFACT_CLOSURE_INDEX:-}"
AO2_RELEASE_READINESS_SUMMARY="${AO2_RELEASE_READINESS_SUMMARY:-}"
AO2_RELEASE_TRAIN_CONTROL_PLANE_BRIDGE_SUMMARY="${AO2_RELEASE_TRAIN_CONTROL_PLANE_BRIDGE_SUMMARY:-}"

require_file() {
  [ -s "$1" ] || { echo "required release source is missing: $1" >&2; exit 1; }
}

rm -rf "$AO2_RELEASE_PUBLICATION_DIR"
mkdir -p "$AO2_RELEASE_PUBLICATION_DIR" "$(dirname "$AO2_RELEASE_PUBLICATION_LIST")"

for record in \
  "macos-aarch64:$ROOT/dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz" \
  "linux-aarch64:$ROOT/dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz" \
  "linux-x86_64:$ROOT/dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz" \
  "windows-x86_64:$ROOT/dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz"; do
  target="${record%%:*}"
  archive="${record#*:}"
  base="ao2-$AO2_VERSION-$target"
  require_file "$archive"
  require_file "$ROOT/dist-provenance/$base.tar.gz.sha256"
  require_file "$ROOT/dist-provenance/$base.tar.gz.sig"
  cp "$archive" "$AO2_RELEASE_PUBLICATION_DIR/$base.tar.gz"
  cp "$ROOT/dist-provenance/$base.tar.gz.sha256" "$AO2_RELEASE_PUBLICATION_DIR/$base.tar.gz.sha256"
  cp "$ROOT/dist-provenance/$base.tar.gz.sig" "$AO2_RELEASE_PUBLICATION_DIR/$base.tar.gz.sig"
  tar -xOzf "$archive" SBOM.cdx.json > "$AO2_RELEASE_PUBLICATION_DIR/$base.sbom.cdx.json"
  require_file "$AO2_RELEASE_PUBLICATION_DIR/$base.sbom.cdx.json"
done

for name in ao2-release-provenance.json ao2-release-provenance.json.sig ao2-release-signing-public.pem; do
  require_file "$ROOT/dist-provenance/$name"
  cp "$ROOT/dist-provenance/$name" "$AO2_RELEASE_PUBLICATION_DIR/$name"
done

for record in \
  "ao2-release-artifact-closure-index.json:$AO2_RELEASE_ARTIFACT_CLOSURE_INDEX" \
  "ao2-release-readiness-summary.json:$AO2_RELEASE_READINESS_SUMMARY" \
  "ao2-release-train-control-plane-bridge-summary.json:$AO2_RELEASE_TRAIN_CONTROL_PLANE_BRIDGE_SUMMARY"; do
  name="${record%%:*}"
  source="${record#*:}"
  require_file "$source"
  cp "$source" "$AO2_RELEASE_PUBLICATION_DIR/$name"
done

(
  cd "$AO2_RELEASE_PUBLICATION_DIR"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print \
    | sed 's#^./##' \
    | LC_ALL=C sort \
    | while IFS= read -r name; do shasum -a 256 "$name"; done \
    > SHA256SUMS
  find . -maxdepth 1 -type f -print \
    | sed 's#^./##' \
    | LC_ALL=C sort \
    > "$AO2_RELEASE_PUBLICATION_LIST"
)

printf 'release_publication_dir=%s\n' "$AO2_RELEASE_PUBLICATION_DIR"
printf 'release_publication_list=%s\n' "$AO2_RELEASE_PUBLICATION_LIST"
printf 'release_publication_assets=23\n'
