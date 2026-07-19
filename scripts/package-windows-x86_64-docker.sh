#!/bin/sh
set -eu

IMAGE="${AO2_WINDOWS_RELEASE_IMAGE:-rust:1.95-bookworm}"
VERSION="$(cargo pkgid -p ao2-cli | sed -e 's/.*#//' -e 's/.*@//')"
CROSS_BINARY="${AO2_WINDOWS_X86_64_BINARY:-target/release-cross/x86_64-pc-windows-gnu/ao2.exe}"
BUILD_COMMIT="${AO2_BUILD_GIT_COMMIT:-$(git rev-parse HEAD)}"
BOUNDARY_JSON="${AO2_WINDOWS_GNU_CROSS_PACKAGE_BOUNDARY_JSON:-dist-windows/cross-package-windows-gnu-from-linux.boundary.json}"

if [ ! -x target/release/ao2 ]; then
  AO2_BUILD_GIT_COMMIT="$BUILD_COMMIT" cargo build --release -p ao2-cli
fi

docker run --rm \
  -v "$PWD":/workspace \
  -w /workspace \
  -e AO2_BUILD_GIT_COMMIT="$BUILD_COMMIT" \
  "$IMAGE" \
  sh -lc '
    set -eu
    export PATH=/usr/local/cargo/bin:$PATH
    export CARGO_TARGET_DIR=/tmp/ao2-target
    apt-get update >/dev/null
    apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 >/dev/null
    toolchain="${RUSTUP_TOOLCHAIN:-$(rustup show active-toolchain)}"
    toolchain="${toolchain%% *}"
    rustup target add --toolchain "$toolchain" x86_64-pc-windows-gnu >/dev/null
    cargo build --release -p ao2-cli --target x86_64-pc-windows-gnu
    mkdir -p "$(dirname "'"$CROSS_BINARY"'")"
    cp /tmp/ao2-target/x86_64-pc-windows-gnu/release/ao2.exe "'"$CROSS_BINARY"'"
  '

AO2_PACKAGED_GIT_COMMIT="$BUILD_COMMIT" AO2_PACKAGED_BUILD_PROFILE=release \
target/release/ao2 release package \
  --out-dir dist-windows \
  --binary "$CROSS_BINARY" \
  --target-label windows-x86_64

docker run --rm \
  -v "$PWD/dist-windows":/dist \
  -w /tmp \
  -e AO2_VERSION="$VERSION" \
  "$IMAGE" \
  sh -lc 'archive="/dist/ao2-${AO2_VERSION}-windows-x86_64.tar.gz" && test -f "$archive" && tar -xzf "$archive" && test -f ./install.ps1 && grep -q "bin/ao2.exe" ./SHA256SUMS && sha256sum "$archive"'

mkdir -p "$(dirname "$BOUNDARY_JSON")"
python3 - "$BOUNDARY_JSON" "$VERSION" "$BUILD_COMMIT" <<'PY'
import json
import sys
from pathlib import Path

boundary = {
    "schema_version": "ao2.cross-package-windows-gnu-from-linux.boundary.v1",
    "status": "passed",
    "classification": "non_authoritative",
    "source": "linux-container-cross-build",
    "target_triple": "x86_64-pc-windows-gnu",
    "target_label": "windows-x86_64",
    "version": sys.argv[2],
    "git_commit": sys.argv[3],
    "canonical_public_windows_archive": False,
    "canonical_public_windows_archive_reason": "native hosted Windows MSVC owns the public Windows archive",
    "canonical_public_windows_archive=false": True,
    "native_hosted_windows_msvc_required": True,
    "native_hosted_windows_msvc_required=true": True,
    "allowed_use": [
        "early_cross_compilation",
        "static_archive_structure_check",
    ],
    "forbidden_use": [
        "public_windows_release_artifact",
        "native_windows_execution_evidence",
        "windows_lifecycle_evidence",
    ],
}
Path(sys.argv[1]).write_text(json.dumps(boundary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
