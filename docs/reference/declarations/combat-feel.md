# Combat presentation and movement

Combat outcomes remain authoritative in `src/sim/game/`; the renderer follows observed attack phases, facing, projectiles and visual cues. Camera motion and visual effects must not change hit timing or expose hidden targets.

## Movement contracts

`motion.ts`, `localPath.ts`, `attackApproach.ts`, `trafficRequests.ts` and `trafficEscape.ts` coordinate continuous positions, navigation corridors, weapon-range approaches and congestion recovery. See the adjacent formation and traffic module notes for implementation details. Moving units must not tunnel through terrain or parked blockers, chase unseen current positions, or repeatedly discard a usable route because of presentation smoothing.

Queued intentions are described in [unit orders](unit-orders.md). [Spatial sectors](../expansion/spatial-sectors.md) bound local queries; [worker execution](../expansion/simulation-worker.md) keeps simulation searches off the render thread. Path searches still execute synchronously inside that worker.

## Presentation contracts

Attack/cast windup, release and recovery follow the shared presentation clock. Ranged visuals originate at declared sockets and follow observed projectile state. Team colors, health bars and selection silhouettes must remain legible at gameplay zoom. Spell appearance is authored independently through [layered effects](../expansion/spell-effects.md).

Inspect mixed-speed formations, crowded final approaches, narrow crossings, moving ranged targets and stop/retarget commands. Save/restore and accelerated local play must retain simulation behavior. Use fresh profiling captures rather than historical build-by-build movement experiments as evidence of current performance.
