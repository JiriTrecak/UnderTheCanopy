# Declarative abilities

Abilities are published content executed by one shared, deterministic interpreter. Usage, authoring and publication: [Spell & Effect Studio](../expansion/spell-effects.md). Field-level semantics: `tooling/spell-editor/agent-app/author-spells/reference/` (in this repository: [the canopy-abilities skill](../../../.claude/skills/canopy-abilities/SKILL.md)).

## Principles

- An ability is data usable by any compatible unit, hero, campaign or neutral AI. Shipped abilities use exactly the same schema and interpreter as any other authored ability. There are no per-spell classes, opcodes or spell-name branches in the simulation, renderer, editor or AI.
- The engine supplies a bounded vocabulary of targeting, operations, statuses and deliveries. No arbitrary code, expressions evaluated as code, imports or recursion. Unsupported definitions are rejected, never approximated.
- Gameplay, presentation and unit bindings are separate. The simulation decides outcomes; animation, sound and particles only illustrate its events.
- Authoring happens in the independent Spell & Effect Studio (`tooling/spell-editor/`). The game and map editor consume published content; workbench UI and authoring-server code stay out of the game bundle.

## Content contracts

**Ability definition** (`src/content/abilities/schema.ts`): name, description, ranks with typed parameter arrays, targeting, costs, cooldown, cast lifecycle, delivery, ordered operation programs, status templates, triggers, AI policy and a presentation reference.

**Operations and statuses**: an operation is a one-time action (heal, damage, status, summon, teleport…); a status is persistent state with duration, stacking, modifiers and bounded lifecycle hooks; a delivery schedules when operations run (direct, projectile, channel, chain, line, aura, persistent zone, swarm). A visual named "fire" does not itself apply burning.

**Unit binding** (`content/game.json`): a unit references an ability through a stable binding ID with initial rank, hero learning requirements, command placement/hotkey, autocast and AI policy. Control permissions (`player`, `ai`, `scenario`) are explicit; a hidden binding can still be active, paid and on cooldown. Orders address the binding. Cooldowns are per caster and ability ID, so duplicate bindings cannot bypass them. Hero progression owns learned ranks; mechanics never know which hero is casting.

**Presentation**: binds published effects by stable ID to lifecycle events, anchors and lifetimes, and maps semantic animation roles (`cast.prepare`, `cast.release`, `cast.recover`, `cast.channel`) to clips and sockets. Missing optional sockets use declared fallbacks; required ones fail binding validation.

**Campaign and map**: campaigns and maps resolve current published definitions by stable ID and carry hero identity, experience and learned ranks across missions; they never copy spell definitions. Mission transitions clear temporary casts, statuses and cooldowns; a mid-mission save preserves them.

## Publication and loading

Each ability and effect has a stable-ID source folder; publication validates schemas, references, budgets and unit bindings, verifies asset bytes and atomically replaces the current published library. A failure leaves the previous publication intact; referenced definitions and assets cannot silently disappear. Consumers always resolve the current publication, with no version pins.

At launch the main thread and the simulation worker receive the same serializable content source. The worker builds and freezes its own `ContentRegistry` and checks its fingerprint against the main thread. Multiplayer requires agreement on `SIMULATION_BUILD` and the published content hash before the first commit. No fetch or publication modifies an active match.

Only the local published library bundled with the game is resolved. Downloadable packs, per-map dependency manifests and remote registries are not implemented; if added, they must remain declarative data with bounded, acyclic, fully resolved dependencies and no script privileges.

## Deterministic runtime

**Commands.** Clients send a binding-addressed intention (`{type: "castAbility", actor, binding, target}`), never damage, costs, ranks or results. The game validates it on the committed tick against its own state; previews and button states are advisory. Player AI runs on every peer through the same validation; neutral controllers use an internal, permission-checked intent source. Mission scripts request casts through the same queue.

**Lifecycle.** `accepted → turning/preparing → released → recovering → finished`, with cancellation before release. Acceptance escrows mana and locks the cooldown; release revalidates caster, target, range, sight and eligibility, then consumes escrow and starts cooldown. Cancellation before release refunds a still-existing caster; nothing after release is refunded or undone. Channels, deliveries and persistent effects own their own clocks and cleanup.

**Ordering and time.** Everything uses simulation ticks (40 per second) and the gameplay clock; no wall clock, render delta, animation event or worker scheduling participates. Due deliveries and combat work resolve in a stable order by tick, phase, source entity, instance, operation index and target entity. Death and invalidation apply before the next queue item. Lifecycle reactions (death, kill and similar triggers) are queued in a saved, bounded FIFO, never resolved recursively in the same call.

**Numbers and budgets.** Integer health, mana and ticks; fixed-point derived modifiers with defined rounding. Selection sorts by declared metric and entity ID before truncation. Random effects derive streams from match seed, cast and operation; visual randomness is separate. Queries, chains, statuses, summons and deliveries have explicit engine limits; admitted work is never silently dropped because a frame is slow.

**State versus presentation.** Binding ranks, cooldowns, escrow, pending casts, deliveries, statuses, summon ownership, corpses, reaction queues and RNG are saved and checksummed; restore validates references and due ticks. Presentation events are observer-filtered before leaving the worker, carry cast and sequence identity, and are excluded from checksums. Late-visible persistent effects reconstruct from their visible state; hidden casters and telegraphs never leak. Clients with reduced or no rendering reach identical checksums.

## Verification

Engine behavior is covered by test-owned fixtures (`tests/fixtures/content`) rather than shipped content. `tests/abilities/` covers schema and publication failures, cast lifecycle, combat integration, lockstep categories across two peers, save/restore mid-cast, visibility filtering, AI scoring and the workbench service. Each new primitive needs deterministic tests and at least two differently authored uses.
