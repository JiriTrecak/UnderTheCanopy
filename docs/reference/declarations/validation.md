# Validation

Run `npm run build` for strict TypeScript and the game bundle, `npm run build:tools` for authoring tools, and `npm test` for simulation, presentation and infrastructure checks. Local socket tests need loopback access. Use `npm run assets:compile` to verify released asset bytes and regenerate deterministic publication indexes. `npm run wiki:check` and `npm run wiki:build` check documentation generation and internal wiki links.

Choose focused tests around the change, then broaden when it affects shared contracts. A passing isolated test is not a claim that the full suite passes. Several older map/model fixtures may need replacement after content retirement; report actual failures rather than copying historical pass counts.

## Required boundaries

- Content schemas reject invalid references and incomplete behavior contracts before use.
- Ownership, costs, prerequisites and placement are checked by the simulation, not trusted from UI.
- Saves round-trip orders, resource reservations, research, hero state and campaign state; incompatible fingerprints fail explicitly.
- AI consumes allowed briefing/observation data and emits ordinary commands.
- Identical inputs produce identical checksums across lockstep peers.
- Rendering must not expose hidden entities or scenery through fog, water, lighting or selection aids.
- Asset publication rejects changed/missing resources, stale revisions and broken dependencies.

Use actual game/editor previews for shader compilation, team colors, animation, bridge traversal and UI interactions. Keep captures and benchmark results in ignored `tmp/`. Record map, view, resolution and graphics settings with measurements; CPU scope timings, asynchronous GPU timings and browser frame intervals are different signals.
