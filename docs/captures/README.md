# Review captures

Side-by-side images for Jiri's review (D24): the old game on the left, the new renderer on the right, same map, camera pose and hour. Produced with `just shot` and the old game's reference stage (`docs/renderer.md` §10).

- `m3-terrain/`: renderer core (terrain, scenery, buildings, lighting, shadows). Units, water and effects come in later steps.
- `m4-ui/`: UI toolkit widget gallery in the old game's style (not yet wired into the game).
- `m3-water/`: water, post effects, surroundings and render scale (M3.4, D34). Old game left, new right: `echo-isles-water` (shallow channels), `echo-isles-coast`, `echo-isles-border` (map corner, turned 45°), `echo-isles-a` (player 1 start), `turtle-rock-water`, `turtle-rock-border` (west edge). New only: `echo-isles-ripples` (12 synthetic bodies wading, `--movers 12`), `echo-isles-forest` (the forest beyond the border in a free 22° view; no play or reference-stage camera can frame it), `echo-isles-a-contact` (contact shading off | on), `echo-isles-a-deep-forest-effects` and `…-autumn-forest-effects` (the map previewed in those biomes, haze and bloom off | on; no shipped map uses them, so there is no old capture), `render-scale-1-vs-075` (2560×1600 at render scale 1 | 0.75).
