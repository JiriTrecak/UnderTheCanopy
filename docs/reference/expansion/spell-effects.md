# Spell & Effect Studio

Abilities and visual effects are declarative published content. The **Spell & Effect Studio** authors, previews and publishes them; the game and map editor only consume the published libraries. Architecture and runtime contracts: [declarative abilities](../design/abilities.md).

Start it with `npm run dev:spells` and open `http://127.0.0.1:5177/`. It does not need the game or map editor. `npm run mcp:spells` starts its stdio MCP server (the editor service must be running).

## Shipped content

`content/abilities/published.json` and `content/effects/published.json` hold the current libraries: 24 abilities and 49 reusable effects, bound to ant, beetle and neutral units in `content/game.json`. Logical library folders live in each directory's `folders.json`. Do not duplicate inventories here. Engine tests never load shipped content: they use test-owned abilities and effects in [tests/fixtures/content](../../tests/fixtures/content/index.ts), so shipped spells can change or be removed without losing engine coverage.

## What the engine supports

The strict schemas (`src/content/abilities/schema.ts`, `src/content/effects/schema.ts`) define the complete vocabulary; unknown operations and fields fail validation. In summary:

- **Targeting and casting:** unit, point and self targeting; ally/enemy/neutral relations; hero, summon, level and unit-nature filters; bounded trait, relation and resource conditions and branches; ranked parameters; mana escrow, preparation, release, recovery, cooldowns, interruption and refunds.
- **Delivery:** direct, channelled waves, homing projectiles, chains, piercing lines, persistent zones and toggles, auras, periodic programs, returning swarms, air/ground heights.
- **Operations:** heal, damage, drain, mana, status, dispel, summon (including ranked and corpse-consuming), resurrect, revive (retained heroes), split (linked multi-unit forms), teleport, vision, convert, contain/release, sacrifice.
- **Statuses and modifiers:** timed or until-death statuses, stacking and refresh, shields, damage/control/spell immunity, concealment and detection, forms (appearance, weapon profile, flight), weapon modifiers (critical strike, evasion, cleave, mana-paid attack bonuses, impact marks), and death, kill, weapon-release and interval triggers.
- **AI and autocast:** observed-information scoring through the same targeting and admission rules as players.

The authoritative field-by-field description is the spell skill reference in `tooling/spell-editor/agent-app/author-spells/reference/` (definition, operations, runtime, presentation, validated examples; in this repository it lives on as [the canopy-abilities skill](../../../.claude/skills/canopy-abilities/SKILL.md)). `studio_schema` serves it as sections `reference` and `reference.<topic>`. Update that reference when engine capabilities change, and re-validate every `json` block against the real schemas (illustrative fragments use `jsonc`).

Out of scope: terrain/destructible interactions, resource-reward operations, modifiers to other abilities' costs or cooldowns, arbitrary actor/socket motion ribbons, downloadable packs and active-match hot replacement.

## Effects and presentation

A spell presentation binds published effects by stable ID to a lifecycle event (for example `released`, `wave`, `statusApplied`), a caster/target anchor and a finite or status lifetime; radius/delivery bindings adapt dimensions to the authoritative event. Presentations never embed visual layers. A presentation may declare a command `icon` asset; unit bindings can override it.

An effect composes up to 32 layers: `ring`, `pillar`, `burst`, `billboard`, `streaks`, `glow`, `light`, `rain`, `missile`, `beam`, `wavefront`, `particles`, `sound` and `mesh`. Layers support textures with flipbooks, ground or camera orientation, motion channels (rotation, scale, opacity), actor sockets, delivery trails, positional audio and published GLB meshes with clips. Budgets live in `src/content/effects/limits.ts`.

`EffectPlayer` renders an effect without any spell dependency; `AbilityEffects` adapts observer-filtered simulation events and statuses to it. Presentation events are excluded from checksummed state, and no effect can mutate gameplay. Cosmetic culling never changes simulation results.

## Workbench

- **Spells:** inspector with quick values and **Configure all attributes** (mechanics and ranks, animations and effect links), plus unit-binding configuration.
- **Effects:** standalone visual stage using the game renderer, biome lighting and resource catalogue; layer editing, undo/redo, preview and publish.
- **Libraries:** folder trees for spells and effects; folder changes preserve IDs and links.
- **Encounter:** an isolated instance of the real `Game` simulation (40 ticks per second) with setup presets, relationships, health, mana, distance, biome, rank, natures, hero/flight overrides, fallen targets, optional combat and interruption tests. Preview casts bypass mana and cooldown only in the cloned preview definition.
- **Transport:** the spell timeline and event inspector use simulation ticks; seeking replays the encounter. Effect playback has its own visual-only timeline.

Save validates and commits a draft against an expected revision. Publish replaces the current definition under its stable ID; running matches keep their loaded content. Concurrent edits are rejected rather than overwritten.

## Publication and loading

Each ability has a stable-ID folder (`content/abilities/<id>/definition.json`, `presentation.json`); each effect has `content/effects/<id>/effect.json`. Textures, audio and meshes stay in the standard asset pipeline and are referenced by asset ID and role/index, never by filename. Publishing verifies resource bytes and rejects missing references; a referenced asset cannot be removed while in use. There are no version pins or historical releases: consumers resolve the current published content.

Startup validates definitions and resource references. Multiplayer peers must agree on `SIMULATION_BUILD` (`src/shared/simulationBuild.ts`) and the SHA-256 identity of published content before the first commit. The worker receives the same serializable source and checks its registry fingerprint.

## Authoring assistant

**Settings** stores an OpenAI API key in the operating-system credential store (no plaintext fallback) and selects the authoring and image models; **Assistant** opens the embedded agent. The key is never returned to the browser, agent, tool results or project files; only the editor backend injects it into OpenAI requests. Chat history stays in ignored `tooling/spell-editor/agent-app/.eve/`. This is a local, loopback-bound, single-user workbench.

The system prompt is `tooling/spell-editor/agent-app/author-spells/SKILL.md`, imported by `agent/instructions.ts`. The five tools (`studio_schema`, `studio_author`, `studio_canvas`, `studio_image`, `studio_asset_image`) share contracts in `tooling/spell-editor/shared/authoringTools.ts` and implementations in `server/authoringTools.ts`; the assistant and MCP call the same services. Generated images keep their source and generation metadata and publish through the asset pipeline; existing asset IDs cannot be overwritten. The assistant has no shell, file or key access.

Server changes require restarting `dev:spells`. Publication reloads the client without restarting the backend.

## MCP workflow

`npm run mcp:spells` exposes `spell_schema` (document and command schemas) and `spell_author {command}` (the same operations as the application).

1. `list`, `catalog`, `read`: keep the returned revision.
2. `validate`, `preview.load` (whole document plus settings), `preview.cast`, `preview.step`: inspect recipients, statuses, events and checksum. Further controls: `preview.beginCast`, `preview.confirmTarget`, `preview.aim`, `preview.seek`, `preview.target`, `preview.state`, `preview.play`, `preview.stop`, `preview.reset`, `preview.replay`, `preview.rank`, `preview.stun`, `preview.displace`, `preview.kill`, `preview.autocast`, `preview.attack`.
3. `save {expectedRevision}`, then `publish` with the returned revision. New documents use `expectedRevision: null` and unique IDs.

```json
{"command":{"op":"read","id":"ability.core.holy-light-lite"}}
```

Effects use `effects.list`, `effects.library`, `effects.read`, `effects.validate`, `effects.save`, `effects.publish` and `effects.preview.*`. Publish effects before the spells that bind them. Folders use `tree.read {kind}` and `tree.mutate {kind, expectedRevision, action}`. Unit bindings use `binding.read` and `bind`, which accept only published abilities.

## Tests and benchmarks

Engine coverage lives in `tests/abilities/` (mechanics, lockstep categories, save/restore, workbench, authoring) and effect renderer tests in `tests/render/`. `scripts/bench/spell-effects/` measures first-cast preparation of published effects.
