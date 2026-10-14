# Graphics settings and performance checks

Main-menu and in-game Settings share persistent resolution and shadow controls. Resolution only changes the 3D render target; HUD text stays at display resolution. Shadow changes apply to an existing renderer immediately and are picked up by newly opened game/editor renderers.

Shadow modes:
- **Soft**: existing VSM appearance and blur passes; default retained.
- **Filtered**: PCF depth comparisons with filtered edges; avoids VSM blur passes and receiver-only shadow draws. Tree/building shadows remain.
- **Off**: no dynamic shadow pass. Authored materials and contact shading remain.

Preference: `utc.graphics.shadows`. Invalid/unavailable storage falls back to Soft. This is presentation state and never enters map files or deterministic simulation snapshots. The debug panel reports the active mode.

Validate changes at a fixed map, camera, time of day and resolution. Compare whole-frame GPU timings and CPU scopes separately; triangle counts and FPS alone do not establish the bottleneck.
