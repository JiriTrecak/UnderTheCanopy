# Status

Where the Rust rebuild stands, written for Jiri after the night of 2026-10-14/15. Details live in the linked docs.

## Try it

```bash
just play                         # main menu → skirmish, campaign, multiplayer, settings, saves
just play --map echo-isles        # straight into a skirmish against the AI
just play --spectate turtle-rock  # watch an AI-versus-AI match
just package                      # build Under the Canopy.app
just bench-render                 # GPU benchmark (add --preset medium for the base M1 preset)
```

The first run cooks the asset pack (about 30 s), later runs reuse it. F9 opens the debug overlay.

## Done

| Milestone | State |
|---|---|
| M0 repository, CI, packaging | done; CI builds, tests and packages macOS and Windows |
| M1 content | done; reads today's formats with parity to the TypeScript loaders |
| M2 simulation | done; every system ported and checked tick by tick against the old game; AI-vs-AI matches play to a winner with stable fingerprints |
| M3 renderer | done; terrain, scenery, water and ripples, post effects, animated units, effects, fog of war, world UI, minimap and portrait, matched against the old game in side-by-side captures |
| M4 playable game | done; controls, HUD, placement, menus, skirmish, campaign screen, mission HUD, settings, key bindings, saves, audio |
| M5 multiplayer | done; the client speaks the frozen protocol and played full matches through the real old server |

[Parity checklist](parity.md): 195 features done, 8 partial, none missing, 9 dropped on purpose (see that page and D41). Late in the night: save export/import (native file dialogs), the old observer panel, mission objectives history, mission cinematics with scripted unit shots, debug speed/reveal/fog perspective, render levels of detail and graphics presets, and the scenery cutaway (giant canopy models open around your units; `m3-effects/scenery-cutaway`).

## Review captures

[captures/](captures/README.md): old game on the left, new on the right where both exist.
- `m3-terrain/`, `m3-water/`: the world.
- `m3-live/`: units, bases, battles.
- `m3-effects/`: spells, fog, world UI, minimap, portrait.
- `m4-play/`, `m4-screens/`: HUD, placement, unit cameras, every screen.

## Decisions to review

Taken overnight with best judgement and marked "review" in [decisions](decisions.md). The ones you'll notice first:
- **D28**: the AI fixes stay (archers keep their distance, real tree sizes, worker counts, no orders for unseen resources, whole-spell scoring).
- **D33/D38**: Georgia and system-ui come from the OS, as in the old game; Outfit is bundled. Settings are tabbed; "Map Editor" became "Load Game"; skirmish has a team choice; the result screen gained statistics.
- **D34**: water look and its two-draw approach; recommended quality is render scale 1 at 1080p and 0.75 on Retina.
- **D35**: the simulation runs one tick ahead of the picture (25 ms more command latency); when it falls behind, the match slows instead of skipping.
- **D36/D41**: right-click on an ally moves; Escape cascades (cancel → back → menu); Cmd also assigns control groups; F9 for debug; command hotkeys are rebindable; the day cycle runs on match time.
- **D40**: one global effect budget; the fog look; the minimap shows the world from above instead of the old palette; progress and cast bars are new.
- **D42**: graphics presets picked from the GPU on first run; a base M1 gets Medium, which renders at most 1440 × 810 and upscales, without contact shading and with low haze.
- **D29/D32**: audio voice budgets; the game reports match outcomes to the server (the old client never did, so its finished rooms stay "playing" forever).

## Known gaps and next steps

- Art fixes for the old editors: [art issues](art-issues.md).
- Remaining parity rows: [parity](parity.md) (filled spell-aim outlines, menu background effects, third-person boom against scenery, debug navigation overlays).
- **Base M1 not measured.** All timings are from an M4 Max (High: worst 4.8 ms a frame; Medium: worst 2.7 ms). Scaled, Medium on a base M1 is about 10 ms in a player view and about 16 ms zoomed out as a spectator, the tight case ([renderer §9](renderer.md), D42). Run `just bench-render --preset medium` on an M1.
- **Windows** builds and passes tests in CI but nobody has played it.
- Phase 2 (editors, server port, Steam, signing, distribution) is unchanged: [plan](plan.md).
