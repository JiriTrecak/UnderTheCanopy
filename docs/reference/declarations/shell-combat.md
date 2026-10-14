# Native shell combat

Status: shared system implemented; actual Bombardier content and assets are still part of the active Tier 2 pass.

`behaviors.combat.shell` extends ordinary attack targeting/range/cooldown with `flightTicks`, `radius`, `slowPermille`, and `slowTicks`. These are balance data. The native simulation owns flight timing, impact eligibility, damage, and effect expiry. Renderer callbacks never deal damage.

On an attack, a shell captures the shooter identity/definition/owner, exact destination, launch/impact tick, resolved attack damage, damage type, blast radius, slow properties and eligible recipient factions. It does not home. A moving victim can leave the blast. A removed shooter does not remove an airborne shell. Ordinary friendly splash is excluded; an explicit force attack can include the chosen friendly faction. Neutral recipients are included when targeted or hostile. Units use subcell positions and structures use footprint distance from the blast center.

Impact enters the normal armor, absorption, death and experience path exactly once. Non-unit structures cannot receive movement slows. Control-immune/invulnerable units reject a newly applied slow. Equal strengths refresh their expiry. Different active strengths use the maximum rather than adding; after a short stronger slow expires, a longer weaker one can remain. Timer cleanup occurs before movement. A captured shell retains its original radius/damage if research completes while it is in flight.

Research effects `splashRadius` and `splashSlowPermille` raise eligible units' future shot parameters. The registry rejects these effects on units without shell combat. Existing health/armor/research declarations remain separate.

Snapshot build 20 saves shells, identity counter and unit slow timers. Restore validates definition capability, timing, world bounds, recipients/viewers and identity. Observations filter shots to launch witnesses; an unrestricted observer sees all. The renderer creates a high arc and a brief expanding impact ring from those observed records in two instanced batches. No new model is implied by this primitive effect.

Verification: `tests/game/shells.test.ts` exercises delay, one-time application, dodging, friendly exclusion, shooter death, overlapping slow expiry, research capture and deterministic restore.

### Authored firing windup

`combat.shell.windupTicks` optionally delays launch while the attack animation plays.
The attack cooldown starts immediately (the renderer's attack trigger); the unit
holds position until release. The Bombardier's 30-frame clip at 24 fps and 1.5×
playback releases at 65%, corresponding to 22 simulation ticks. The target's
position is captured at release, after which the projectile does not home.
Player interruption, stun, spellcasting, target death or lost visibility cancels
an unreleased shot; the spent cooldown remains. Released shells survive their
shooter. Pending windups are saved and validated. Simulation build: 21.
