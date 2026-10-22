#!/bin/sh
# Turns generated campaign art (the old repository's .asset-work/concepts/buildings/<slug>-1.png,
# made with tools/buildings/concept.sh) into interface assets of a campaign:
#   tools/campaign/art.sh <campaign> <slug>:<name> ...
# writes base/assets/interface/campaign/<campaign>/<name>/{asset.json,image.webp} (1920 wide).
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
MAIN=$(cd "$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)
SRC="$MAIN/../Settlers 3 Web/.asset-work/concepts/buildings"
CAMPAIGN=$1; shift
for pair in "$@"; do
  slug=${pair%%:*}; name=${pair#*:}
  dir="$ROOT/base/assets/interface/campaign/$CAMPAIGN/$name"
  mkdir -p "$dir"
  cwebp -quiet -q 84 -resize 1920 0 "$SRC/$slug-1.png" -o "$dir/image.webp"
  cat > "$dir/asset.json" <<J
{
  "id": "asset.interface.campaign.$CAMPAIGN.$name",
  "name": "$CAMPAIGN campaign: $name",
  "kind": "interface",
  "tags": ["campaign", "$CAMPAIGN", "original"],
  "resources": [{"role": "image", "index": 1, "format": "webp"}],
  "license": "Original artwork for Under the Canopy, generated with gpt-image-2."
}
J
  # The editor's one file format: 2-space JSON.
  python3 -c "import json,sys; p=sys.argv[1]; d=json.load(open(p)); open(p,'w').write(json.dumps(d, indent=2) + '\\n')" "$dir/asset.json"
  echo "$name"
done
