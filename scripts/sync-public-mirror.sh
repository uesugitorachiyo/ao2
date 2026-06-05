#!/bin/sh
set -eu

# sync-public-mirror.sh — overlay a sanitized export of an ao-*-private
# repository onto its ao-* public mirror, then commit + push.
#
# Design contract:
#   1. The sanitization is owned by `<source>/scripts/public_clean_export.py`.
#      It writes a PUBLIC-EXPORT-MANIFEST.json with `verdict` and
#      `blocked_patterns`. We refuse to sync anything but verdict=PASS with
#      zero blocked_patterns.
#   2. The overlay uses rsync without `--delete`, so files the public
#      mirror has accumulated outside the private source (community
#      translations, GitHub-issue templates, etc.) are preserved. Anything
#      the export removes from the private source must be deleted in the
#      public mirror manually — this script never deletes.
#   3. Commits use a stable public-mirror identity so contributors can
#      filter them. Pushes target the mirror's `origin` (the public repo).
#      We hard-block any source-side push from this script — the private
#      origin is owned by the private repo's own release pipeline.
#   4. The script is idempotent. If the overlay produces no diff after
#      `git pull --ff-only`, no commit or push happens and the script
#      reports `mirror_changed=false`.
#
# Inputs (env or flags):
#   --source <dir>          Path to the private repo (must contain scripts/public_clean_export.py)
#   --target <dir>          Path to the public mirror checkout (must be a git repo with `origin` set)
#   --label <name>          Short name used in the commit message + summary line (e.g. "ao-runtime")
#   --dry-run               Run the export + overlay but skip commit and push.
#   --commit-author <name>  Override the public commit identity (default: AO Public Mirror Bot)
#   --commit-email <addr>   Override the public commit email (default: the user's noreply email)
#   --preserve <relpath>    Repeatable. Skip overlay for this path (rsync --exclude). Use for
#                           public-only files like README.md when the public mirror has community
#                           additions (language selectors, contribution guides) the private source
#                           doesn't carry.
#
# Output (stdout summary, machine-parseable key=value lines):
#   mirror_sync_label=<label>
#   mirror_sync_verdict=PASS
#   mirror_sync_blocked_patterns=0
#   mirror_sync_target=<absolute path>
#   mirror_sync_changed=<true|false>
#   mirror_sync_pushed=<true|false>
#   mirror_sync_head=<sha>
#   mirror_sync=passed

SOURCE_DIR=""
TARGET_DIR=""
LABEL=""
DRY_RUN=0
COMMIT_AUTHOR="${SYNC_PUBLIC_MIRROR_AUTHOR:-AO Public Mirror Bot}"
COMMIT_EMAIL="${SYNC_PUBLIC_MIRROR_EMAIL:-270548076+uesugitorachiyo@users.noreply.github.com}"
PRESERVE_PATHS=""

while [ $# -gt 0 ]; do
  case "$1" in
    --source) SOURCE_DIR="$2"; shift 2 ;;
    --target) TARGET_DIR="$2"; shift 2 ;;
    --label) LABEL="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --commit-author) COMMIT_AUTHOR="$2"; shift 2 ;;
    --commit-email) COMMIT_EMAIL="$2"; shift 2 ;;
    --preserve)
      if [ -z "$PRESERVE_PATHS" ]; then PRESERVE_PATHS="$2"; else PRESERVE_PATHS="$PRESERVE_PATHS|$2"; fi
      shift 2 ;;
    *) echo "sync-public-mirror.sh: unknown flag $1" >&2; exit 2 ;;
  esac
done

if [ -z "$SOURCE_DIR" ] || [ -z "$TARGET_DIR" ] || [ -z "$LABEL" ]; then
  echo "sync-public-mirror.sh: --source, --target, --label are required" >&2
  exit 2
fi
if [ ! -d "$SOURCE_DIR" ]; then
  echo "sync-public-mirror.sh: source directory does not exist: $SOURCE_DIR" >&2
  exit 2
fi
if [ ! -f "$SOURCE_DIR/scripts/public_clean_export.py" ]; then
  echo "sync-public-mirror.sh: $SOURCE_DIR has no scripts/public_clean_export.py" >&2
  exit 2
fi
if [ ! -d "$TARGET_DIR/.git" ]; then
  echo "sync-public-mirror.sh: target is not a git checkout: $TARGET_DIR" >&2
  exit 2
fi

SOURCE_DIR="$(cd "$SOURCE_DIR" && pwd)"
TARGET_DIR="$(cd "$TARGET_DIR" && pwd)"

TARGET_REMOTE="$(cd "$TARGET_DIR" && git remote get-url origin 2>/dev/null || true)"
if [ -z "$TARGET_REMOTE" ]; then
  echo "sync-public-mirror.sh: target $TARGET_DIR has no origin remote" >&2
  exit 2
fi
# Refuse to push to anything ending in -private.git — that would leak the
# private source identity into the public mirror remote and is almost
# certainly a misconfiguration.
case "$TARGET_REMOTE" in
  *-private.git|*-private)
    echo "sync-public-mirror.sh: target origin is a private repo ($TARGET_REMOTE); refusing to sync" >&2
    exit 2
    ;;
esac

EXPORT_DIR="$(mktemp -d -t sync-public-mirror-XXXXXX)"
trap 'rm -rf "$EXPORT_DIR"' EXIT

echo "[$LABEL] running clean export: source=$SOURCE_DIR target=$EXPORT_DIR" >&2
(cd "$SOURCE_DIR" && python3 scripts/public_clean_export.py --target "$EXPORT_DIR")

MANIFEST="$EXPORT_DIR/PUBLIC-EXPORT-MANIFEST.json"
if [ ! -f "$MANIFEST" ]; then
  echo "[$LABEL] export did not write PUBLIC-EXPORT-MANIFEST.json" >&2
  exit 1
fi
VERDICT="$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('verdict',''))" "$MANIFEST")"
BLOCKED_COUNT="$(python3 -c "import json,sys; m=json.load(open(sys.argv[1])); v=m.get('blocked_patterns',[]); print(len(v) if isinstance(v,list) else (int(v) if isinstance(v,int) else 1))" "$MANIFEST")"
if [ "$VERDICT" != "PASS" ] || [ "$BLOCKED_COUNT" != "0" ]; then
  echo "[$LABEL] clean export refused: verdict=$VERDICT blocked_patterns=$BLOCKED_COUNT" >&2
  exit 1
fi

echo "[$LABEL] pulling target up to date: $TARGET_DIR" >&2
(cd "$TARGET_DIR" && git fetch --quiet origin && git checkout --quiet main && git pull --quiet --ff-only origin main)

echo "[$LABEL] rsync overlay (no --delete) → $TARGET_DIR" >&2
RSYNC_EXTRA=""
if [ -n "$PRESERVE_PATHS" ]; then
  OLD_IFS="$IFS"
  IFS='|'
  for path in $PRESERVE_PATHS; do
    RSYNC_EXTRA="$RSYNC_EXTRA --exclude=$path"
    echo "[$LABEL] preserving public file: $path" >&2
  done
  IFS="$OLD_IFS"
fi
# shellcheck disable=SC2086
rsync -a --exclude='.git/' $RSYNC_EXTRA "$EXPORT_DIR/" "$TARGET_DIR/"

cd "$TARGET_DIR"
CHANGED=false
PUSHED=false
if [ -n "$(git status --porcelain)" ]; then
  CHANGED=true
  if [ "$DRY_RUN" = "1" ]; then
    echo "[$LABEL] dry-run: skipping commit + push (diff stat below)" >&2
    git --no-pager diff --stat | sed 's/^/[dry-run] /' >&2
  else
    git add -A
    COMMIT_MSG="sync($LABEL): mirror sanitized export

Overlay produced by scripts/sync-public-mirror.sh from the private
$LABEL repository's scripts/public_clean_export.py
(verdict=PASS, blocked_patterns=0). Files outside the sanitized
export are preserved (no --delete).
"
    GIT_AUTHOR_NAME="$COMMIT_AUTHOR" \
    GIT_AUTHOR_EMAIL="$COMMIT_EMAIL" \
    GIT_COMMITTER_NAME="$COMMIT_AUTHOR" \
    GIT_COMMITTER_EMAIL="$COMMIT_EMAIL" \
      git commit --quiet -m "$COMMIT_MSG"
    git push --quiet origin main
    PUSHED=true
  fi
fi

HEAD_SHA="$(git rev-parse HEAD)"
cd - >/dev/null

printf "mirror_sync_label=%s\n" "$LABEL"
printf "mirror_sync_verdict=%s\n" "$VERDICT"
printf "mirror_sync_blocked_patterns=%s\n" "$BLOCKED_COUNT"
printf "mirror_sync_target=%s\n" "$TARGET_DIR"
printf "mirror_sync_changed=%s\n" "$CHANGED"
printf "mirror_sync_pushed=%s\n" "$PUSHED"
printf "mirror_sync_head=%s\n" "$HEAD_SHA"
printf "mirror_sync=passed\n"
