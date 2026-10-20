---
name: canopy-icons
description: The rules and the pipeline for every Under the Canopy icon (abilities, passives, research and upgrade lines, commands, stats, unit, hero, creature and building portraits, items, resources) — Jiri's approved style (D76), the per-type rules, tools/icons/icons.py (requests, candidates, pick, recompose, mark), the review tool with yay/nay votes, and checking icons in the game. Use before making, repainting, reviewing or wiring any icon, for new content too.
---

# Icons

Read **canopy-assets** first (asset folders, ids, cooking). Background and history: [art direction §Icons](../../../docs/art-direction.md#icons) and D76 in [decisions](../../../docs/decisions.md).

An icon must read at a glance at HUD size and show exactly what the thing does: Warcraft III's painted close-ups with StarCraft II's discipline. Jiri: "clean icons that exactly represent what is happening, with the right amount of detail, but not so much that they are not readable".

## Sizes

At 1080p the command card shows an icon at 46 px, a group's unit cards at about 34 px, the hero bar at 58 px, tooltip costs at 24 px and status badges at 22 px; HiDPI doubles them. Design for 32 px. Icons ship as 256 × 256 PNG (the cooker makes mips).

## The rules

1. **One subject, one idea.** The object, body part or effect that does it, in close-up: Storm Bolt is the thrown hammer, Thunder Clap the hoof striking the ground. Never the caster's whole body, a battle scene or a crowd of small figures; no scenery or ground.
2. **A big silhouette**, about 70 % of the frame, readable as a black shape alone. Chunky forms; no filigree, thin strands, sparkle or fine texture.
3. **One warm key light** from the upper left. The subject's lit side is the brightest thing in the icon, and a thin rim separates it from the background.
4. **Painted transparent.** The subject and its own glow on real alpha; the tool supplies the background. Never describe a background in a subject.
5. **Two or three colours.** Effects only where the effect is the subject, as one bold shape (one fork, one splash, one flame).
6. **Each icon is unlike its neighbours.** A caster's abilities differ in silhouette and dominant colour, not only in subject. An icon must not echo another one on the same card or a similar icon elsewhere (a banner like another race's banner, painted rings next to a ring-shaped icon).
7. **Calm corners.** Bottom-right holds the hotkey; top-left holds the research diamonds or the HUD's passive mark. No text, letters, numbers, runes, frame or border in any icon.
8. **Straight perspective** for symmetric objects (crowns, emblems, medallions): seen from the front, level, not tilted.

## Per type

| `kind` in the spec | Subject | Background (tool) | Marks |
|---|---|---|---|
| `ability` (active, autocast, toggle) | the acting object, diagonal; a glow of its energy colour close around it | `ability`, near-black violet | — |
| `passive` (passive, aura) | a still, frontal, symmetric emblem; top-left kept calm | `passive`, near-black | none in the image: the HUD draws a white pulse mark top-left and no hotkey |
| `research` (beetles), `research-grown` (ants) | one improved part as a still life, diagonal | `research` steel, `research-grown` umber | `level` 1–3 gold diamonds top-left |
| `command` (and stats) | one near-monochrome steel symbol, the same look for all commands | `command`, near-black slate | — |
| `unit`, `machine`, `hero` | the head about half the frame, three-quarter view, one signature feature named; a warm (hero: golden) glow behind the head that is part of the icon | `portrait`, near-black warm | — |
| `building` | the whole building simplified to its two or three biggest features, no ground or plinth | `building`, near-black warm | — |
| `item` | the object, diagonal, a faint glow of its own colour | `item`, near-black | — |
| `resource` | the resource with a thin dark outline | none: a cut-out for the resource ribbon | — |

Backgrounds are nearly invisible on purpose (Jiri found even a dark slate too light). They live in `BACKGROUNDS` in `tools/icons/icons.py`; changing one and running `recompose` changes every icon of that type without repainting.

## Palettes and energy colours

- **Ants:** glossy sand-and-bronze chitin, ivory mandibles, big black eyes, royal-blue silk and plates, gold for royalty, amber resin. Everything grown or secreted. Substance is a violet **liquid**, never crystals.
- **Beetles:** glossy black chitin, dark riveted steel, brass, royal-blue plates. Violet substance only inside glass, orange furnace fire, white steam.
- **Neutral creatures:** their own natural materials; corrupted ones show rot and magenta-violet spore light.
- **Items:** forest-made (amber, bark, chitin, silk, bone, leaves, gold); light items feel light (woven grass and silk, not metal).
- **Energy colours say what it does:** violet substance and mana, orange fire, forge and impact, warm gold healing, auras and royalty, steel-white armour and blocking, white steam. **Never green magic.**

## Upgrade lines get better, not different

Every level of a research line, and any pair like Build → Build Advanced, is the **same object in the same pose and size**, only better made:
- beetles: plain iron → polished steel and brass → royal-blue enamel with a violet substance-glass core;
- ants: plain chitin → hardened, glossier, with gold or amber → royal, with violet substance veins.

Level 1 has the full subject and says what it is not yet made of. Each next level has `"after": <previous name>` and only what improves; the tool paints it from the previous level's picked image. Pick level 1 before requesting level 2. Each level gets its own icon folder and id.

## The pipeline

Images come from GPT Image 2.5 Sunburst (`MODEL` in icons.py) with a transparent background, through `tools/buildings/concept.sh` and the old repository's stored key. **Never print or copy the key.** References are paths inside the old repository (`.asset-work/concepts/…`); outputs land there as `icon-<name>-<n>.png`.

```bash
PY="$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)"
"$PY" tools/icons/icons.py requests tools/icons/specs/<spec>.json <scratch>/req [name ...]
ls <scratch>/req/*.json | xargs -P 4 -n 1 sh tools/buildings/concept.sh
"$PY" tools/icons/icons.py candidates tools/icons/specs/<spec>.json <scratch>/sheet.png [name ...]
"$PY" tools/icons/icons.py pick tools/icons/specs/<spec>.json <name>=<n> ...
"$PY" tools/icons/icons.py recompose [folder prefix]    # after a background change
"$PY" tools/icons/icons.py mark                         # redraws the HUD's passive mark
```

- **Specs** (`tools/icons/specs/`, one file per race and type) hold one item per icon: `{name, kind, faction, dir, subject, refs?, refnote?, after?, level?, background?, id?, title?, tags?, race?}`. `dir` is the icon's asset folder; a new folder also needs `id` (`asset.icons.ability-<race>-<caster>-<ability>`, `asset.icons.research-<race>-<research>`, `asset.portraits.units.<race>-<unit>`, `asset.portraits.buildings.<race>-<building>`) and `title`. Names are unique across all specs.
- **Subjects** start with the NAME in capitals and a colon, then one or two concrete sentences: the object, its pose and angle, its two or three materials or colours, what to leave out ("Only the bolt: no caster."), and for abilities "Energy colour: …". Read the ability's or research's description first and take the most direct picture of what it does. A concept image as the first ref keeps the design; its `refnote` says which part to draw and what not to.
- **`requests`** without names writes requests only for items not picked yet; name items to repaint them. Each request makes two candidates.
- **Choose** from the `candidates` sheet at 46 px, not at 160: the one that reads first, and for chains the one that keeps the previous level's pose.
- **`pick`** installs the choice: `source.png` (the painted foreground, a working file), `generation.json` (the request and the choices) and `image.png` composed over the type's background with its marks. An existing folder keeps its id, name and tags.
- **Wire** new icons in content: an ability's `presentation.icon` (and a unit's ability binding `icon` where it has one), a research's `icon`, a unit's or building's `icon`, a command's `icon` in `base/rules/commands.json`. One thing, one icon: never two different icons for the same ability. Remove a folder nothing references any more.

## Check

- `cargo test -q -p canopy-content --test shipped_data`.
- In the game at real size: `just shot --play --map ant-bench --input "focus:<unit>;key:K;ticks:2" --size 1920x1080 --out <png>` (or `beetle-bench`) (K opens a hero's skills; `select:unit.ants` shows the group's unit cards). Crop the command card and look at it at 1:1.
- Scratch (request folders, sheets, captures) stays in the scratchpad and is deleted when the icons are in. Note player-visible icon changes with `just release-note --kind improved`.

## Review with Jiri

The review tool is `tools/icons/review.html`, published as the artifact https://claude.ai/artifact/2vmFUCd9qUEcWZvAMxLxtT with its own vote store. It shows every icon in use at 128, 46 and 22 px with what uses it, a Yay, a "Nay: repaint" and a comment.
- Build its data with `python3 tools/icons/review.py <scratch>/review` and republish `review.html` to that URL with `root` = that folder and `files` = `manifest.json` plus the changed `icons/<id>.png` (unchanged files stay published).
- Read the votes with the ArtifactData tool: `list` on collection `reviews` (one document per icon id: `verdict`, `comment`, `version`). A vote counts only while its `version` matches the icon's current image hash; a repainted icon asks for a new vote and keeps its old comment.
- Repaint each nay from its comment (rewrite the subject in the spec, then request, pick, check, republish). A nay without a comment usually means it echoes another icon or does not read: find which, and say what you changed.
- Ask Jiri in prose, never with question boxes. Visible changes to the rules here need his agreement (AGENTS.md).

## What Jiri's votes taught

- The ability's own words decide the picture: Forgefall drops an anvil, not a crucible.
- Upgrade pairs are one object getting better (Build → Build Advanced is the same hammer, finer), not two different objects.
- Passives get no frame, ring or tinted background: the HUD's white pulse mark is the whole sign, so an aura toggle can dim it later.
- Glows belong to the icon; background colour does not.
- A portrait that is busy at 34 px loses its ornaments: one head, one signature prop.
- Symmetric objects are drawn straight; items called quick or light look light and forest-made.
