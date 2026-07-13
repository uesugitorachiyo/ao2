#!/bin/sh
set -eu

# Guarded end-to-end AO2 release publisher.
AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$AO2_VERSION}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_RELEASE_CHANNEL="${AO2_RELEASE_CHANNEL:-}"
AO2_RELEASE_TITLE="${AO2_RELEASE_TITLE:-}"
AO2_RELEASE_NOTES_FILE="${AO2_RELEASE_NOTES_FILE:-}"
AO2_RELEASE_SHIP_CONFIRM="${AO2_RELEASE_SHIP_CONFIRM:-}"
AO2_RELEASE_SHIP_DRY_RUN="${AO2_RELEASE_SHIP_DRY_RUN:-0}"
AO2_RELEASE_EXPECTED_ASSET_MANIFEST="${AO2_RELEASE_EXPECTED_ASSET_MANIFEST:-}"
AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256="${AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256:-}"
AO2_RELEASE_TARGET_COMMIT="${AO2_RELEASE_TARGET_COMMIT:-$(git rev-parse HEAD)}"
AO2_RELEASE_PUBLICATION_DIR="${AO2_RELEASE_PUBLICATION_DIR:-target/release-publication/$AO2_RELEASE_TAG}"
AO2_RELEASE_PUBLICATION_LIST="${AO2_RELEASE_PUBLICATION_LIST:-target/release-publication/$AO2_RELEASE_TAG.assets.txt}"
AO2_RELEASE_DOWNLOAD_DIR="${AO2_RELEASE_DOWNLOAD_DIR:-target/release-download/$AO2_RELEASE_TAG}"
AO2_RELEASE_DOCTOR_JSON="${AO2_RELEASE_DOCTOR_JSON:-$AO2_RELEASE_DOWNLOAD_DIR/release-doctor.json}"
AO2_RELEASE_COMPARISON_DIR="${AO2_RELEASE_COMPARISON_DIR:-target/release-comparison-bundles}"
AO2_RELEASE_COMPARISON_RESULT="${AO2_RELEASE_COMPARISON_RESULT:-$AO2_RELEASE_DOWNLOAD_DIR/release-comparison-result.json}"
AO2_RELEASE_COMPARISON_VERIFICATION="${AO2_RELEASE_COMPARISON_VERIFICATION:-$AO2_RELEASE_DOWNLOAD_DIR/release-comparison-verification.json}"
AO2_RELEASE_COMPARISON_SIGNING_KEY="${AO2_RELEASE_COMPARISON_SIGNING_KEY:-.release-signing/ao2-release-signing-key.pem}"
AO2_RELEASE_COMPARISON_SIGNER_ID="${AO2_RELEASE_COMPARISON_SIGNER_ID:-release-ship}"
AO2_RELEASE_CODEX_PILOT_ACCEPTANCE="${AO2_RELEASE_CODEX_PILOT_ACCEPTANCE:-0}"
AO2_RELEASE_CODEX_PILOT_REQUIRED="${AO2_RELEASE_CODEX_PILOT_REQUIRED:-1}"
AO2_RELEASE_CODEX_PILOT_ROOT="${AO2_RELEASE_CODEX_PILOT_ROOT:-target/provider-pilot-acceptance/$AO2_RELEASE_TAG}"
AO2_RELEASE_CODEX_PILOT_BIN="${AO2_RELEASE_CODEX_PILOT_BIN:-target/release/ao2}"
AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE="${AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE:-0}"
AO2_RELEASE_CLAUDE_PILOT_REQUIRED="${AO2_RELEASE_CLAUDE_PILOT_REQUIRED:-1}"
AO2_RELEASE_CLAUDE_PILOT_ROOT="${AO2_RELEASE_CLAUDE_PILOT_ROOT:-target/provider-pilot-acceptance/$AO2_RELEASE_TAG/claude}"
AO2_RELEASE_CLAUDE_PILOT_BIN="${AO2_RELEASE_CLAUDE_PILOT_BIN:-target/release/ao2}"
AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE="${AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE:-0}"
AO2_RELEASE_ANTIGRAVITY_PILOT_REQUIRED="${AO2_RELEASE_ANTIGRAVITY_PILOT_REQUIRED:-1}"
AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT="${AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT:-target/provider-pilot-acceptance/$AO2_RELEASE_TAG/antigravity}"
AO2_RELEASE_ANTIGRAVITY_PILOT_BIN="${AO2_RELEASE_ANTIGRAVITY_PILOT_BIN:-target/release/ao2}"
AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD="${AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD:-1.00}"
AO2_RELEASE_RETENTION_KEEP_RELEASES="${AO2_RELEASE_RETENTION_KEEP_RELEASES:-3}"
AO2_RELEASE_RETENTION_KEEP_BUNDLES="${AO2_RELEASE_RETENTION_KEEP_BUNDLES:-3}"
AO2_RELEASE_RETENTION_PRUNE="${AO2_RELEASE_RETENTION_PRUNE:-1}"
AO2_WORKBENCH_RELEASE_COMPARISON_ROOT="${AO2_WORKBENCH_RELEASE_COMPARISON_ROOT:-$AO2_RELEASE_DOWNLOAD_DIR/workbench-release-comparison-export-smoke}"
AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON="${AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON:-$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT/workbench-release-comparison-export.json}"
AO2_WORKBENCH_PROVIDER_PILOT_ROOT="${AO2_WORKBENCH_PROVIDER_PILOT_ROOT:-$AO2_RELEASE_DOWNLOAD_DIR/workbench-provider-pilot-acceptance-export-smoke}"
AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="${AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE:-}"
AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON="${AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON:-$AO2_WORKBENCH_PROVIDER_PILOT_ROOT/workbench-provider-pilot-acceptance-export.json}"
AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_ROOT="${AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_ROOT:-$AO2_RELEASE_DOWNLOAD_DIR/workbench-claude-provider-pilot-acceptance-export-smoke}"
AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_EXPORT_JSON="${AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_EXPORT_JSON:-$AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_ROOT/workbench-claude-provider-pilot-acceptance-export.json}"
AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_ROOT="${AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_ROOT:-$AO2_RELEASE_DOWNLOAD_DIR/workbench-antigravity-provider-pilot-acceptance-export-smoke}"
AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_EXPORT_JSON="${AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_EXPORT_JSON:-$AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_ROOT/workbench-antigravity-provider-pilot-acceptance-export.json}"
AO2_UBUNTU_SSH_TARGET="${AO2_UBUNTU_SSH_TARGET:-ao2-ubuntu-nucx}"
AO2_WINDOWS_SSH_TARGET="${AO2_WINDOWS_SSH_TARGET:-win-hp255-via-ubuntu}"
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE="${AO2_REQUIRE_NATIVE_WINDOWS_SMOKE:-1}"

if [ "$AO2_RELEASE_SHIP_DRY_RUN" != "1" ] && [ "$AO2_RELEASE_SHIP_CONFIRM" != "ship-$AO2_RELEASE_TAG" ]; then
  echo "refusing to publish release; set AO2_RELEASE_SHIP_CONFIRM=ship-$AO2_RELEASE_TAG" >&2
  exit 1
fi

if [ "$AO2_RELEASE_SHIP_DRY_RUN" != "1" ]; then
  if [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST" ] \
    || [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" ]; then
    echo "live publication requires AO2_RELEASE_EXPECTED_ASSET_MANIFEST and AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" >&2
    exit 1
  fi
  AO2_RELEASE_APPROVAL_BOUND=1
elif [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST" ] \
  && [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" ]; then
  AO2_RELEASE_APPROVAL_BOUND=0
elif [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST" ] \
  || [ -z "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" ]; then
  echo "dry run requires both expected asset manifest variables or neither" >&2
  exit 1
else
  AO2_RELEASE_APPROVAL_BOUND=1
fi

if [ "$AO2_RELEASE_TARGET_COMMIT" != "$(git rev-parse HEAD)" ]; then
  echo "refusing to publish unreviewed moving head; expected $AO2_RELEASE_TARGET_COMMIT, observed $(git rev-parse HEAD)" >&2
  exit 1
fi

AO2_RELEASE_CONTRACT_REQUIRE_ASSETS=0 \
AO2_RELEASE_CHANNEL="$AO2_RELEASE_CHANNEL" \
AO2_RELEASE_TITLE="$AO2_RELEASE_TITLE" \
AO2_RELEASE_NOTES_FILE="$AO2_RELEASE_NOTES_FILE" \
AO2_RELEASE_CODEX_PILOT_ACCEPTANCE="$AO2_RELEASE_CODEX_PILOT_ACCEPTANCE" \
AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE="$AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE" \
AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE="$AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE" \
  scripts/release-publication-contract.sh

if [ "${AO2_RELEASE_SHIP_ALLOW_DIRTY:-0}" != "1" ] && [ -n "$(git status --porcelain)" ]; then
  echo "refusing to publish release from dirty worktree; commit first or set AO2_RELEASE_SHIP_ALLOW_DIRTY=1" >&2
  git status --short >&2
  exit 1
fi

AO2_RELEASE_RETENTION_KEEP_RELEASES="$AO2_RELEASE_RETENTION_KEEP_RELEASES" \
AO2_RELEASE_RETENTION_KEEP_BUNDLES="$AO2_RELEASE_RETENTION_KEEP_BUNDLES" \
AO2_RELEASE_RETENTION_PRUNE="$AO2_RELEASE_RETENTION_PRUNE" \
  npm run release:retention-preflight

npm run verify
npm run release:build-all

AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
AO2_WINDOWS_SSH_TARGET="$AO2_WINDOWS_SSH_TARGET" \
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE="$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" \
  npm run smoke:three-os

npm run release:gate

AO2_RELEASE_PUBLICATION_DIR="$AO2_RELEASE_PUBLICATION_DIR" \
AO2_RELEASE_PUBLICATION_LIST="$AO2_RELEASE_PUBLICATION_LIST" \
  scripts/release-stage-publication-assets.sh

AO2_RELEASE_PUBLICATION_DIR="$AO2_RELEASE_PUBLICATION_DIR" \
AO2_RELEASE_CHANNEL="$AO2_RELEASE_CHANNEL" \
AO2_RELEASE_TITLE="$AO2_RELEASE_TITLE" \
AO2_RELEASE_NOTES_FILE="$AO2_RELEASE_NOTES_FILE" \
AO2_RELEASE_CODEX_PILOT_ACCEPTANCE="$AO2_RELEASE_CODEX_PILOT_ACCEPTANCE" \
AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE="$AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE" \
AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE="$AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE" \
  scripts/release-publication-contract.sh

if [ "$AO2_RELEASE_APPROVAL_BOUND" = "1" ]; then
  approved_asset_verification=$(
    python3 scripts/release-verify-approved-assets.py \
      --manifest "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST" \
      --manifest-sha256 "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256" \
      --publication-dir "$AO2_RELEASE_PUBLICATION_DIR" \
      --publication-list "$AO2_RELEASE_PUBLICATION_LIST"
  )
  printf '%s\n' "$approved_asset_verification"
fi

if [ "$AO2_RELEASE_CODEX_PILOT_ACCEPTANCE" = "1" ]; then
  AO2_BIN="$AO2_RELEASE_CODEX_PILOT_BIN" \
  AO2_CODEX_PROVIDER_PILOT_ROOT="$AO2_RELEASE_CODEX_PILOT_ROOT" \
  AO2_PROVIDER_PILOT_MAX_BUDGET_USD="$AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD" \
  AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION="$AO2_VERSION" \
  AO2_LIVE_CODEX_PILOT=1 \
  AO2_LIVE_CODEX_PILOT_REQUIRED="$AO2_RELEASE_CODEX_PILOT_REQUIRED" \
    npm run smoke:provider:codex-pilot
  printf "release_codex_provider_pilot_acceptance=%s\n" "$AO2_RELEASE_CODEX_PILOT_ROOT/provider-pilot-acceptance.json"
  printf "release_codex_provider_pilot_acceptance=passed\n"
fi

if [ "$AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE" = "1" ]; then
  AO2_BIN="$AO2_RELEASE_CLAUDE_PILOT_BIN" \
  AO2_CLAUDE_PROVIDER_PILOT_ROOT="$AO2_RELEASE_CLAUDE_PILOT_ROOT" \
  AO2_PROVIDER_PILOT_MAX_BUDGET_USD="$AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD" \
  AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION="$AO2_VERSION" \
  AO2_LIVE_CLAUDE_PILOT=1 \
  AO2_LIVE_CLAUDE_PILOT_REQUIRED="$AO2_RELEASE_CLAUDE_PILOT_REQUIRED" \
    npm run smoke:provider:claude-pilot
  printf "release_claude_provider_pilot_acceptance=%s\n" "$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json"
  printf "release_claude_provider_pilot_acceptance=passed\n"
fi

if [ "$AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE" = "1" ]; then
  AO2_BIN="$AO2_RELEASE_ANTIGRAVITY_PILOT_BIN" \
  AO2_ANTIGRAVITY_PROVIDER_PILOT_ROOT="$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT" \
  AO2_PROVIDER_PILOT_MAX_BUDGET_USD="$AO2_RELEASE_PROVIDER_PILOT_MAX_BUDGET_USD" \
  AO2_PROVIDER_PILOT_RELEASE_CANDIDATE_VERSION="$AO2_VERSION" \
  AO2_LIVE_ANTIGRAVITY_PILOT=1 \
  AO2_LIVE_ANTIGRAVITY_PILOT_REQUIRED="$AO2_RELEASE_ANTIGRAVITY_PILOT_REQUIRED" \
    npm run smoke:provider:antigravity-pilot
  printf "release_antigravity_provider_pilot_acceptance=%s\n" "$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json"
  printf "release_antigravity_provider_pilot_acceptance=passed\n"
fi

if git rev-parse "refs/tags/$AO2_RELEASE_TAG" >/dev/null 2>&1 \
  || git ls-remote --exit-code --tags origin "refs/tags/$AO2_RELEASE_TAG" >/dev/null 2>&1; then
  echo "refusing to reuse existing release tag: $AO2_RELEASE_TAG" >&2
  exit 1
fi

if gh release view "$AO2_RELEASE_TAG" --repo "$AO2_RELEASE_REPO" >/dev/null 2>&1; then
  echo "refusing to overwrite existing release: $AO2_RELEASE_TAG" >&2
  exit 1
fi

if [ "$AO2_RELEASE_SHIP_DRY_RUN" = "1" ]; then
  if [ "$AO2_RELEASE_APPROVAL_BOUND" = "1" ]; then
    printf "release_approval_bound=true\n"
    printf "release_approved_asset_manifest_sha256=%s\n" "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"
  else
    printf "release_approval_bound=false\n"
    printf "release_approved_asset_manifest_sha256=not_supplied\n"
  fi
  printf "release_ship_dry_run=passed\n"
  printf "release_ship_mutations=not_executed\n"
  printf "release_ship_target_commit=%s\n" "$AO2_RELEASE_TARGET_COMMIT"
  printf "release_ship_asset_list=%s\n" "$AO2_RELEASE_PUBLICATION_LIST"
  exit 0
fi

git tag -a "$AO2_RELEASE_TAG" "$AO2_RELEASE_TARGET_COMMIT" -m "$AO2_RELEASE_TITLE"
git push origin "$AO2_RELEASE_TAG"

set --
while IFS= read -r asset; do
  [ -n "$asset" ] || continue
  set -- "$@" "$AO2_RELEASE_PUBLICATION_DIR/$asset"
done < "$AO2_RELEASE_PUBLICATION_LIST"

if [ "$AO2_RELEASE_CHANNEL" = "prerelease" ]; then
  gh release create "$AO2_RELEASE_TAG" "$@" \
    --repo "$AO2_RELEASE_REPO" \
    --verify-tag \
    --title "$AO2_RELEASE_TITLE" \
    --notes-file "$AO2_RELEASE_NOTES_FILE" \
    --prerelease \
    --latest=false
else
  gh release create "$AO2_RELEASE_TAG" "$@" \
    --repo "$AO2_RELEASE_REPO" \
    --verify-tag \
    --title "$AO2_RELEASE_TITLE" \
    --notes-file "$AO2_RELEASE_NOTES_FILE" \
    --latest
fi

AO2_RELEASE_TAG="$AO2_RELEASE_TAG" \
AO2_RELEASE_REPO="$AO2_RELEASE_REPO" \
AO2_RELEASE_DOWNLOAD_DIR="$AO2_RELEASE_DOWNLOAD_DIR" \
AO2_NATIVE_UBUNTU_DOWNLOAD_VERIFY=1 \
AO2_NATIVE_WINDOWS_DOWNLOAD_VERIFY=1 \
AO2_RELEASE_ROLLBACK_VERIFY=1 \
AO2_UBUNTU_SSH_TARGET="$AO2_UBUNTU_SSH_TARGET" \
AO2_WINDOWS_SSH_TARGET="$AO2_WINDOWS_SSH_TARGET" \
  npm run release:download-verify

mkdir -p "$AO2_RELEASE_DOWNLOAD_DIR"
cargo run -p ao2-cli --quiet -- doctor --json --release "$AO2_RELEASE_TAG" --release-asset-dir "$AO2_RELEASE_DOWNLOAD_DIR" --provenance-dir "$AO2_RELEASE_DOWNLOAD_DIR" --release-repo "$AO2_RELEASE_REPO" > "$AO2_RELEASE_DOCTOR_JSON"

grep -q '"assets_available": true' "$AO2_RELEASE_DOCTOR_JSON"
grep -q '"provenance_verified": true' "$AO2_RELEASE_DOCTOR_JSON"
grep -q '"provenance_tag_matches": true' "$AO2_RELEASE_DOCTOR_JSON"

if [ ! -f "$AO2_RELEASE_COMPARISON_SIGNING_KEY" ]; then
  echo "release comparison signing key is missing: $AO2_RELEASE_COMPARISON_SIGNING_KEY" >&2
  exit 1
fi

mkdir -p "$AO2_RELEASE_COMPARISON_DIR"
cargo run -p ao2-cli --quiet -- release compare \
  --release-download-dir "$(dirname "$AO2_RELEASE_DOWNLOAD_DIR")" \
  --out-dir "$AO2_RELEASE_COMPARISON_DIR" \
  --signing-key "$AO2_RELEASE_COMPARISON_SIGNING_KEY" \
  --signer-id "$AO2_RELEASE_COMPARISON_SIGNER_ID" \
  --json > "$AO2_RELEASE_COMPARISON_RESULT"

release_comparison_bundle_dir="$(awk -F '"' '/"bundle_dir"/ { print $4; exit }' "$AO2_RELEASE_COMPARISON_RESULT")"
if [ -z "$release_comparison_bundle_dir" ]; then
  echo "release comparison bundle_dir missing from $AO2_RELEASE_COMPARISON_RESULT" >&2
  exit 1
fi

cargo run -p ao2-cli --quiet -- release compare-verify \
  --bundle-dir "$release_comparison_bundle_dir" \
  --json > "$AO2_RELEASE_COMPARISON_VERIFICATION"

grep -q '"status": "verified"' "$AO2_RELEASE_COMPARISON_VERIFICATION"
grep -q '"manifest_verified": true' "$AO2_RELEASE_COMPARISON_VERIFICATION"
grep -q '"signature_verified": true' "$AO2_RELEASE_COMPARISON_VERIFICATION"

AO2_WORKBENCH_RELEASE_COMPARISON_ROOT="$AO2_WORKBENCH_RELEASE_COMPARISON_ROOT" \
AO2_WORKBENCH_RELEASE_COMPARISON_BUNDLE_DIR="$release_comparison_bundle_dir" \
AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON="$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON" \
  npm run smoke:workbench-release-comparison-export

if [ -z "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ] \
  && [ -f "$AO2_RELEASE_CODEX_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_RELEASE_CODEX_PILOT_ROOT/provider-pilot-acceptance.json"
fi

if [ -z "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ] \
  && [ -f "$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json"
fi

if [ -z "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ] \
  && [ -f "$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json"
fi

if [ -n "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ROOT="$AO2_WORKBENCH_PROVIDER_PILOT_ROOT" \
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" \
  AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON="$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON" \
    npm run smoke:workbench-provider-pilot-acceptance-export
  printf "workbench_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
  printf "workbench_provider_pilot_acceptance_export=passed\n"
  if [ "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" = "$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
    printf "workbench_claude_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
    printf "workbench_claude_provider_pilot_acceptance_export=passed\n"
  fi
  if [ "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" = "$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
    printf "workbench_antigravity_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON"
    printf "workbench_antigravity_provider_pilot_acceptance_export=passed\n"
  fi
fi

if [ "$AO2_RELEASE_CLAUDE_PILOT_ACCEPTANCE" = "1" ] \
  && [ -f "$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json" ] \
  && [ "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" != "$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ROOT="$AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_ROOT" \
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_RELEASE_CLAUDE_PILOT_ROOT/provider-pilot-acceptance.json" \
  AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON="$AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_EXPORT_JSON" \
    npm run smoke:workbench-provider-pilot-acceptance-export
  printf "workbench_claude_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_CLAUDE_PROVIDER_PILOT_EXPORT_JSON"
  printf "workbench_claude_provider_pilot_acceptance_export=passed\n"
fi

if [ "$AO2_RELEASE_ANTIGRAVITY_PILOT_ACCEPTANCE" = "1" ] \
  && [ -f "$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json" ] \
  && [ "$AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE" != "$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json" ]; then
  AO2_WORKBENCH_PROVIDER_PILOT_ROOT="$AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_ROOT" \
  AO2_WORKBENCH_PROVIDER_PILOT_ACCEPTANCE_BUNDLE="$AO2_RELEASE_ANTIGRAVITY_PILOT_ROOT/provider-pilot-acceptance.json" \
  AO2_WORKBENCH_PROVIDER_PILOT_EXPORT_JSON="$AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_EXPORT_JSON" \
    npm run smoke:workbench-provider-pilot-acceptance-export
  printf "workbench_antigravity_provider_pilot_acceptance_export=%s\n" "$AO2_WORKBENCH_ANTIGRAVITY_PROVIDER_PILOT_EXPORT_JSON"
  printf "workbench_antigravity_provider_pilot_acceptance_export=passed\n"
fi

if [ "${AO2_SYNC_PUBLIC_MIRRORS:-0}" = "1" ]; then
  echo "[release-ship] syncing configured public mirrors (AO2_SYNC_PUBLIC_MIRRORS=1)"
  AO2_PUBLIC_MIRROR_DRY_RUN="${AO2_PUBLIC_MIRROR_DRY_RUN:-0}"
  mirror_extra=""
  if [ "$AO2_PUBLIC_MIRROR_DRY_RUN" = "1" ]; then mirror_extra="--dry-run"; fi
  # Default mirror pair set. Each pair runs only if both --source and
  # --target are checked out on this host; otherwise it is skipped with a
  # log line so a partial workstation doesn't fail the ship.
  AO2_SYNC_SECURE_AGENT_PROFILE_SOURCE="${AO2_SYNC_SECURE_AGENT_PROFILE_SOURCE:-/tmp/ao2-public/secure-agent-profile}"
  AO2_SYNC_SECURE_AGENT_PROFILE_TARGET="${AO2_SYNC_SECURE_AGENT_PROFILE_TARGET:-../secure-agent-profile-public}"
  AO2_SYNC_FINANCIAL_SERVICES_PROFILE_SOURCE="${AO2_SYNC_FINANCIAL_SERVICES_PROFILE_SOURCE:-../financial-services-profile}"
  AO2_SYNC_FINANCIAL_SERVICES_PROFILE_TARGET="${AO2_SYNC_FINANCIAL_SERVICES_PROFILE_TARGET:-../financial-services-profile-public}"
  mirror_pushed_count=0
  mirror_run_pair() {
    label="$1"; src="$2"; tgt="$3"; shift 3
    if [ ! -d "$src" ] || [ ! -d "$tgt" ]; then
      echo "[release-ship] skipping mirror $label (source=$src or target=$tgt missing)"
      printf "mirror_sync_%s=skipped\n" "$label"
      return 0
    fi
    if scripts/sync-public-mirror.sh --source "$src" --target "$tgt" --label "$label" $mirror_extra "$@"; then
      mirror_pushed_count=$((mirror_pushed_count + 1))
    else
      echo "[release-ship] mirror sync failed for $label" >&2
      exit 1
    fi
  }
  # secure-agent-profile and financial-services-profile each carry a public-only
  # README language selector that the private export would clobber, so README.md
  # is preserved on those.
  mirror_run_pair secure-agent-profile "$AO2_SYNC_SECURE_AGENT_PROFILE_SOURCE" "$AO2_SYNC_SECURE_AGENT_PROFILE_TARGET" --preserve README.md
  mirror_run_pair financial-services-profile "$AO2_SYNC_FINANCIAL_SERVICES_PROFILE_SOURCE" "$AO2_SYNC_FINANCIAL_SERVICES_PROFILE_TARGET" --preserve README.md
  printf "public_mirror_sync_attempted=%d\n" "$mirror_pushed_count"
  printf "public_mirror_sync=passed\n"
else
  echo "[release-ship] AO2_SYNC_PUBLIC_MIRRORS != 1 — skipping public mirror sync"
  printf "public_mirror_sync=skipped\n"
fi

printf "release_ship_tag=%s\n" "$AO2_RELEASE_TAG"
printf "release_ship_repo=%s\n" "$AO2_RELEASE_REPO"
printf "release_ship_doctor=%s\n" "$AO2_RELEASE_DOCTOR_JSON"
printf "release_comparison_result=%s\n" "$AO2_RELEASE_COMPARISON_RESULT"
printf "release_comparison_verification=%s\n" "$AO2_RELEASE_COMPARISON_VERIFICATION"
printf "release_comparison_bundle_dir=%s\n" "$release_comparison_bundle_dir"
printf "release_comparison_verify=passed\n"
printf "workbench_release_comparison_export=%s\n" "$AO2_WORKBENCH_RELEASE_COMPARISON_EXPORT_JSON"
printf "workbench_release_comparison_export=passed\n"
printf "release_approval_bound=true\n"
printf "release_approved_asset_manifest_sha256=%s\n" "$AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256"
printf "release_ship=passed\n"
