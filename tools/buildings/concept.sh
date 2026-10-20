#!/bin/sh
# Generates one concept request (concept.mts) from the old repository's root, logging next to
# the request: tools/buildings/concept.sh <request.json>. Runs in parallel with xargs -P 4.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
# The old repository sits beside the main checkout (also when this runs from a worktree).
MAIN=$(cd "$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)
OLD="$MAIN/../Settlers 3 Web"
REQ=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
cd "$OLD"
node --import tsx "$HERE/concept.mts" "$REQ" > "$REQ.log" 2>&1 && echo "$(basename "$REQ") ok" || echo "$(basename "$REQ") failed: see $REQ.log"
