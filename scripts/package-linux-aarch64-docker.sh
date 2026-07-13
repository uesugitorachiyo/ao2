#!/bin/sh
set -eu

IMAGE="${AO2_LINUX_RELEASE_IMAGE:-rust:1.95-bookworm}"
BUILD_PLATFORM="${AO2_LINUX_AARCH64_BUILD_DOCKER_PLATFORM:-linux/amd64}"
RUN_PLATFORM="${AO2_LINUX_AARCH64_RUN_DOCKER_PLATFORM:-linux/arm64}"
VERSION="$(cargo pkgid -p ao2-cli | sed -e 's/.*#//' -e 's/.*@//')"
CROSS_BINARY="${AO2_LINUX_AARCH64_BINARY:-target/release-cross/aarch64-unknown-linux-gnu/ao2}"
BUILD_COMMIT="${AO2_BUILD_GIT_COMMIT:-$(git rev-parse HEAD)}"

if [ ! -x target/release/ao2 ]; then
  AO2_BUILD_GIT_COMMIT="$BUILD_COMMIT" cargo build --release -p ao2-cli
fi

docker run --rm \
  --platform "$BUILD_PLATFORM" \
  -v "$PWD":/workspace \
  -w /workspace \
  -e AO2_BUILD_GIT_COMMIT="$BUILD_COMMIT" \
  "$IMAGE" \
  sh -lc '
    set -eu
    export PATH=/usr/local/cargo/bin:$PATH
    export CARGO_TARGET_DIR=/tmp/ao2-target
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
    export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
    apt-get update >/dev/null
    apt-get install -y --no-install-recommends \
      gcc-aarch64-linux-gnu \
      libc6-dev-arm64-cross >/dev/null
    toolchain="${RUSTUP_TOOLCHAIN:-$(rustup show active-toolchain)}"
    toolchain="${toolchain%% *}"
    rustup target add --toolchain "$toolchain" aarch64-unknown-linux-gnu >/dev/null
    cargo build --release -p ao2-cli --target aarch64-unknown-linux-gnu
    mkdir -p "$(dirname "'"$CROSS_BINARY"'")"
    cp /tmp/ao2-target/aarch64-unknown-linux-gnu/release/ao2 "'"$CROSS_BINARY"'"
  '

AO2_PACKAGED_GIT_COMMIT="$BUILD_COMMIT" AO2_PACKAGED_BUILD_PROFILE=release \
target/release/ao2 release package \
  --out-dir dist-linux \
  --binary "$CROSS_BINARY" \
  --target-label linux-aarch64

docker run --rm \
  --platform "$RUN_PLATFORM" \
  -v "$PWD/dist-linux":/dist \
  -w /tmp \
  -e AO2_VERSION="$VERSION" \
  "$IMAGE" \
  sh -lc 'archive="/dist/ao2-${AO2_VERSION}-linux-aarch64.tar.gz" && test -f "$archive" && tar -xzf "$archive" && ./bin/ao2 --help >/dev/null && test -x ./install.sh && grep -q "bin/ao2" ./SHA256SUMS && sha256sum "$archive"'
