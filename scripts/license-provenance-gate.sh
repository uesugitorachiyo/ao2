#!/bin/sh
set -eu

grep_file() {
  pattern="$1"
  path="$2"
  if command -v rg >/dev/null 2>&1; then
    rg -q "$pattern" "$path"
  else
    grep -F -q "$pattern" "$path"
  fi
}

grep_file 'license = "MIT OR Apache-2.0"' Cargo.toml
grep_file '"license": "MIT OR Apache-2.0"' package.json
test -f LICENSE
test -f LICENSE-MIT
test -f LICENSE-APACHE
test -f docs/THIRD-PARTY-LICENSES.md

cargo metadata --format-version 1 > target/ao2-license-metadata.json
node -e '
const fs = require("fs");
const metadata = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const forbidden = /(^|[^AL])GPL|AGPL|MPL|SSPL|Commons Clause/i;
const offenders = metadata.packages
  .map((pkg) => [pkg.name, pkg.version, pkg.license || "NOASSERTION"])
  .filter(([, , license]) => forbidden.test(license));
if (offenders.length) {
  console.error("forbidden dependency license family detected");
  for (const row of offenders) {
    console.error(row.join("\t"));
  }
  process.exit(1);
}
' target/ao2-license-metadata.json

AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
if [ -f "$AO2_RELEASE_PROVENANCE_DIR/ao2-release-provenance.json" ]; then
  npm run release:verify-provenance >/dev/null
elif [ "${AO2_REQUIRE_RELEASE_PROVENANCE:-0}" = "1" ]; then
  echo "missing signed release provenance in $AO2_RELEASE_PROVENANCE_DIR" >&2
  exit 1
else
  printf "release_provenance_verify=skipped_missing_signed_provenance\n"
fi

printf "license_provenance_gate=passed\n"
