#!/usr/bin/env bash
# Mirrors the data the old TypeScript repository's editors author: content/, assets/ and
# art/. Copies only files tracked there, removes files it no longer tracks, and leaves the
# result for review in `git status`. The formats are identical in both repositories during
# phase 1 (docs/plan.md).
set -euo pipefail

old=$(cd "${1:?usage: sync-data.sh <old repository>}" && pwd)
new=$(cd "$(dirname "$0")/.." && pwd)
dirs=(content assets art)
skip='^content/import/'   # Warcraft importer settings belong to the map editor (phase 2)

wanted=$(mktemp); present=$(mktemp)
trap 'rm -f "$wanted" "$present"' EXIT
git -C "$old" ls-files -- "${dirs[@]}" | grep -v "$skip" | sort > "$wanted"
(cd "$new" && find "${dirs[@]}" -type f ! -name .DS_Store | sort) > "$present"

rsync -a --files-from="$wanted" "$old/" "$new/"
comm -13 "$wanted" "$present" | while IFS= read -r stale; do rm -- "$new/$stale"; done
find "${dirs[@]/#/$new/}" -type d -empty -delete

git -C "$new" status --short -- "${dirs[@]}" | awk '{print $1}' | sort | uniq -c
