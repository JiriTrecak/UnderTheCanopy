# Documentation

- [Agent context and code index](agent-context.md): shared Codex/Claude orientation after the Rust rewrite, current baseline, code entry points and repository skills.
- [Status](status.md): where the rebuild stands, how to try it, decisions to review.
- [Plan](plan.md): phase 1 milestones, targets, what is deferred.
- [Decisions](decisions.md): the decision log.
- [Parity](parity.md): the phase 1 checklist against the old game, with evidence and what remains.
- [Structure](structure.md): repository layout, dependency rules, data.
- [Editor](editor.md): the native editor's design, decided milestone by milestone (phase 2).
- [Packages](packages.md): how content is organised: packages, layering, assets, maps (D53). Agent skills in `.claude/skills/canopy-*` describe each format.
- [Art direction](art-direction.md): the graphical direction (D61), its research and the order of work.
- [Renderer](renderer.md): renderer design and its open questions.
- [Audio](audio.md): playback, positional sound, voice limiting, effect sounds and music.
- [UI](ui.md): the HUD and menu toolkit: layout files, theme, widgets, renderer contract.
- [Play](play.md): playing a match: controls, HUD bindings, placement, world UI contract, debug overlay, test harness.
- [Art issues](art-issues.md): problems in the shipped art still to fix.
- [Reference](reference/README.md): documentation of the old TypeScript game: the behaviour specification for the port. Its code paths refer to the old repository.
- [Ability and effect capabilities](../.claude/skills/canopy-abilities/SKILL.md): the agent skill for abilities and effects: every capability the spell system supports, with checked examples.
