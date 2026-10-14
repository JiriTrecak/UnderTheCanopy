# Unit cameras

Selected-unit camera controls and mission Lua shots share the same camera modes.

Select a visible living unit and click the spyglass in the command card, or press **J**. It cycles **RTS → third person → first person → RTS**. Escape returns to RTS after cancelling an active targeting command. Rebind “Cycle camera” in Keyboard shortcuts. The action, icon and default binding are declared in `content/game.json`; abilities retain their assigned bottom-row slots. Observers can use the same camera while inspecting visible units.

Close-up views follow the unit's rendered position and facing, including its current walk surface. They retain normal RTS orders; the camera does not steer the unit. Losing the subject, selecting a building or clearing selection restores RTS. The original RTS focus and zoom are retained. Loading a save starts in RTS. Player camera choices stay local and never become simulation actions.

Ant render assets declare `cameraAnchor` with eye height, forward offset and third-person distance. Other units fall back to a height derived from their declared health-label height. Third person shortens its boom against terrain, static scenery and observed buildings. First person suppresses its own model's color/depth draw while preserving animated shadows and other instances sharing the same materials. Health/mana bars retain a fixed CSS-pixel size in all camera modes; other nearby sprite labels have a screen-size cap. The overhead scenery-cutaway mask is disabled in close views to avoid cutting holes into the floor; interior maps use a dark backdrop.

## Mission shots

The Mission & Lua editor documents the shared API:

```lua
mission.camera('third-person', 'marshal', 'watch-captain', 6, 1.6, 55, 0.5)
mission.say('Marshal', 'unit.ants.marshal', 'We found them.', 4, true)
-- In the next stage, after the dialogue finishes:
mission.end_scene()
```

Arguments are mode, subject Script ID, optional look-at unit Script ID, trailing distance, eye/aim height, vertical field of view, and transition seconds. Use `nil` for defaults. Modes are `rts`, `third-person`, and `first-person`; distance is 1–30, height 0.2–30, FOV 25–90°, transition 0–5 seconds. Distance only affects third person. `mission.camera` starts a scene; `mission.end_scene` releases control and restores the player's view. Existing `begin_scene(x,y)` remains available for RTS staging. Script errors and match outcomes also release the camera. Missing/dead/unobserved subjects fall back safely to RTS.

Shot declarations are validated and saved in mission state, so scripted timing is deterministic across peers and save/load. Rendering and camera interpolation remain local. Shots use the existing cinematic visibility rules; switching a player camera does not reveal fog.


## Interior ceilings

Environment → Interior ceiling height authors a minimum underside height in world metres (1–128). Leave the field empty to remove it. MCP `landscape` environment operations accept `ceilingHeight`, with `null` clearing the setting. The ceiling uses a gently ribbed heartwood surface, renders only in first/third person, and follows normal fog visibility. It is decorative enclosure, not another walkable floor. Keep the authored clearance above every deck and actor; third-person booms shorten before crossing the roof.


## Validation and limits

Automated checks cover facing, raised-floor eye height, camera cycling, RTS restoration, bounded minimap rays, body/shadow draw separation, fixed ability slots, Lua validation and deterministic shot save/restore. Verify both views, scene cleanup and RTS restoration in the current map.

These cameras reveal the existing RTS models at close range. Low-poly faces and simplified foliage are consequently more apparent. Third-person obstruction uses a short center boom rather than a swept camera volume; tight corners and rapid shot transitions may still need shot-specific framing. Free look, direct character steering, arbitrary point look-at shots and bone-mounted eye bob are not part of this version.
