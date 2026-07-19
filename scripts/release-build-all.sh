#!/bin/sh
set -eu

npm run build:release
npm run package:local
npm run package:linux:aarch64:docker
npm run package:linux:x86_64:docker
npm run cross-package:windows:gnu:from-linux
npm run release:sign-provenance
npm run release:verify-provenance

printf "release_build_all=passed\n"
