#!/bin/sh
set -eu

AO2_RELEASE_RETENTION_PRUNE="${AO2_RELEASE_RETENTION_PRUNE:-1}"
AO2_RELEASE_RETENTION_KEEP_RELEASES="${AO2_RELEASE_RETENTION_KEEP_RELEASES:-3}"
AO2_RELEASE_RETENTION_KEEP_BUNDLES="${AO2_RELEASE_RETENTION_KEEP_BUNDLES:-3}"
AO2_RELEASE_RETENTION_RELEASE_DIR="${AO2_RELEASE_RETENTION_RELEASE_DIR:-target/release-download}"
AO2_RELEASE_RETENTION_BUNDLE_DIR="${AO2_RELEASE_RETENTION_BUNDLE_DIR:-target/release-comparison-bundles}"

case "$AO2_RELEASE_RETENTION_PRUNE" in
  0 | 1) ;;
  *)
    echo "AO2_RELEASE_RETENTION_PRUNE must be 0 or 1" >&2
    exit 1
    ;;
esac

case "$AO2_RELEASE_RETENTION_KEEP_RELEASES" in
  '' | *[!0-9]*)
    echo "AO2_RELEASE_RETENTION_KEEP_RELEASES must be a positive integer" >&2
    exit 1
    ;;
esac

case "$AO2_RELEASE_RETENTION_KEEP_BUNDLES" in
  '' | *[!0-9]*)
    echo "AO2_RELEASE_RETENTION_KEEP_BUNDLES must be a positive integer" >&2
    exit 1
    ;;
esac

if [ "$AO2_RELEASE_RETENTION_KEEP_RELEASES" -lt 1 ] || [ "$AO2_RELEASE_RETENTION_KEEP_BUNDLES" -lt 1 ]; then
  echo "release retention keep counts must be greater than 0" >&2
  exit 1
fi

stat_mtime() {
  stat -f "%m" "$1" 2>/dev/null || stat -c "%Y" "$1"
}

prune_matching_dirs() {
  root="$1"
  pattern="$2"
  keep="$3"
  label="$4"
  listing="$(mktemp "${TMPDIR:-/tmp}/ao2-retention.XXXXXX")"
  sorted="$(mktemp "${TMPDIR:-/tmp}/ao2-retention-sorted.XXXXXX")"

  if [ -d "$root" ]; then
    find "$root" -mindepth 1 -maxdepth 1 -type d -name "$pattern" -print | while IFS= read -r path; do
      printf "%s\t%s\n" "$(stat_mtime "$path")" "$path"
    done > "$listing"
  else
    : > "$listing"
  fi

  sort -rn "$listing" | cut -f 2- > "$sorted"
  count="$(wc -l < "$sorted" | tr -d ' ')"
  removed=0
  index=0

  while IFS= read -r path; do
    [ -n "$path" ] || continue
    index=$((index + 1))
    if [ "$index" -le "$keep" ]; then
      continue
    fi
    case "$path" in
      "$root"/*) ;;
      *)
        echo "refusing to prune unexpected $label path: $path" >&2
        exit 1
        ;;
    esac
    if [ "$AO2_RELEASE_RETENTION_PRUNE" = "1" ]; then
      rm -rf -- "$path"
    fi
    removed=$((removed + 1))
    printf "release_retention_removed_%s=%s\n" "$label" "$path"
  done < "$sorted"

  rm -f "$listing" "$sorted"
  printf "release_retention_%s_count=%s\n" "$label" "$count"
  printf "release_retention_%s_removed_count=%s\n" "$label" "$removed"
  return "$removed"
}

set +e
prune_matching_dirs "$AO2_RELEASE_RETENTION_RELEASE_DIR" "v*" "$AO2_RELEASE_RETENTION_KEEP_RELEASES" "release"
release_removed="$?"
prune_matching_dirs "$AO2_RELEASE_RETENTION_BUNDLE_DIR" "release-comparison-*" "$AO2_RELEASE_RETENTION_KEEP_BUNDLES" "bundle"
bundle_removed="$?"
set -e

removed_total=$((release_removed + bundle_removed))
printf "release_retention_release_dir=%s\n" "$AO2_RELEASE_RETENTION_RELEASE_DIR"
printf "release_retention_bundle_dir=%s\n" "$AO2_RELEASE_RETENTION_BUNDLE_DIR"
printf "release_retention_prune=%s\n" "$AO2_RELEASE_RETENTION_PRUNE"
printf "release_retention_keep_releases=%s\n" "$AO2_RELEASE_RETENTION_KEEP_RELEASES"
printf "release_retention_keep_bundles=%s\n" "$AO2_RELEASE_RETENTION_KEEP_BUNDLES"
printf "release_retention_removed_total=%s\n" "$removed_total"
df -k . | awk 'NR == 2 { printf "release_retention_available_kib=%s\n", $4 }'
printf "release_retention_preflight=passed\n"
