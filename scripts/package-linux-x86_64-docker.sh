#!/bin/sh
set -eu

IMAGE="${AO2_LINUX_X86_64_RELEASE_IMAGE:-rust:1.95-bookworm}"
PLATFORM="${AO2_LINUX_X86_64_DOCKER_PLATFORM:-linux/amd64}"
AO2_LINUX_X86_64_CARGO_BUILD_JOBS="${AO2_LINUX_X86_64_CARGO_BUILD_JOBS:-1}"
VERSION="$(cargo pkgid -p ao2-cli | sed -e 's/.*#//' -e 's/.*@//')"
AO2_LINUX_X86_64_SSH_TARGET="${AO2_LINUX_X86_64_SSH_TARGET:-${AO2_UBUNTU_SSH_TARGET:-}}"
AO2_LINUX_X86_64_REMOTE_ROOT="${AO2_LINUX_X86_64_REMOTE_ROOT:-/tmp/ao2-linux-x86_64-package}"
BUILD_COMMIT="${AO2_BUILD_GIT_COMMIT:-$(git rev-parse HEAD)}"

if [ -n "$AO2_LINUX_X86_64_SSH_TARGET" ]; then
  remote_src="$AO2_LINUX_X86_64_REMOTE_ROOT/src"
  remote_dist="$remote_src/dist-linux-x86_64"
  ssh "$AO2_LINUX_X86_64_SSH_TARGET" "rm -rf '$remote_src' && mkdir -p '$remote_src'"
  tar \
    --exclude .git \
    --exclude target \
    --exclude dist \
    --exclude dist-linux \
    --exclude dist-linux-x86_64 \
    --exclude dist-windows \
    --exclude dist-provenance \
    -czf - . | ssh "$AO2_LINUX_X86_64_SSH_TARGET" "tar -xzf - -C '$remote_src'"
  ssh "$AO2_LINUX_X86_64_SSH_TARGET" "cd '$remote_src' && export PATH=\"\$HOME/.cargo/bin:\$PATH\" AO2_BUILD_GIT_COMMIT='$BUILD_COMMIT' && cargo build --release -p ao2-cli && AO2_PACKAGED_GIT_COMMIT='$BUILD_COMMIT' AO2_PACKAGED_BUILD_PROFILE=release target/release/ao2 release package --out-dir dist-linux-x86_64 --target-label linux-x86_64 && archive='$remote_dist/ao2-${VERSION}-linux-x86_64.tar.gz' && test -f \"\$archive\" && tmp=\$(mktemp -d) && tar -xzf \"\$archive\" -C \"\$tmp\" && \"\$tmp/bin/ao2\" --help >/dev/null && test -x \"\$tmp/install.sh\" && grep -q 'bin/ao2' \"\$tmp/SHA256SUMS\" && sha256sum \"\$archive\""
  mkdir -p dist-linux-x86_64
  scp "$AO2_LINUX_X86_64_SSH_TARGET:$remote_dist/ao2-${VERSION}-linux-x86_64.tar.gz" \
    "$AO2_LINUX_X86_64_SSH_TARGET:$remote_dist/SHA256SUMS" \
    dist-linux-x86_64/
  exit 0
fi

docker run --rm \
  --platform "$PLATFORM" \
  -v "$PWD":/workspace \
  -w /workspace \
  -e CARGO_BUILD_JOBS="$AO2_LINUX_X86_64_CARGO_BUILD_JOBS" \
  -e CARGO_INCREMENTAL=0 \
  -e AO2_BUILD_GIT_COMMIT="$BUILD_COMMIT" \
  -e AO2_PACKAGED_GIT_COMMIT="$BUILD_COMMIT" \
  -e AO2_PACKAGED_BUILD_PROFILE=release \
  "$IMAGE" \
  sh -lc 'export CARGO_TARGET_DIR=/tmp/ao2-target && /usr/local/cargo/bin/cargo build --release -p ao2-cli && /tmp/ao2-target/release/ao2 release package --out-dir dist-linux-x86_64 --target-label linux-x86_64'

docker run --rm \
  --platform "$PLATFORM" \
  -v "$PWD/dist-linux-x86_64":/dist \
  -w /tmp \
  -e AO2_VERSION="$VERSION" \
  "$IMAGE" \
  sh -lc 'archive="/dist/ao2-${AO2_VERSION}-linux-x86_64.tar.gz" && test -f "$archive" && tar -xzf "$archive" && ./bin/ao2 --help >/dev/null && test -x ./install.sh && grep -q "bin/ao2" ./SHA256SUMS && sha256sum "$archive"'
