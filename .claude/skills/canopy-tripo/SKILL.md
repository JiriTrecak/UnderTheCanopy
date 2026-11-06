---
name: canopy-tripo
description: How to drive Tripo Studio for Under the Canopy without asking Jiri for anything — Claude in Chrome on his signed-in account, uploading reference images, the generation settings that keep a model true to its concept (one front image, full detail, quads), Retopo down to the triangle budget, segmenting, exporting GLBs through Chrome's downloads, credits and timings. Use for every Tripo model: units, heroes, equipment, buildings, props and map pieces.
---

# Tripo

Every model in the game starts in Tripo from an approved concept (skill **canopy-buildings** for buildings, **canopy-units** for units, heroes and their equipment). This is how to run it end to end yourself. Jiri does not upload, click or export anything: never ask him to.

## The browser

Use **Claude in Chrome** (`mcp__claude-in-chrome__*`, skill anthropic-skills:chrome-browser): Jiri's Chrome is signed in to Tripo. Load `tabs_context_mcp`, `navigate`, `computer`, `find`, `file_upload`, `javascript_tool` and `tabs_close_mcp` in one ToolSearch, open your own tab and close it at the end. The built-in browser pane can't upload or download files; don't use it for Tripo.

- The workspace is `https://studio.tripo3d.ai/workspace/generate` (the old `www.tripo3d.ai/workspace/...` answers 404). Each model gets its own URL ending in its task id.
- The credit balance is in the header. Image to 3D costs 35, Retopo 40, Segment 40. Generation takes 2–3 minutes and the asset list shows a countdown; Retopo takes about 30 seconds. Several generations can run at once.
- Keep `javascript_tool` calls short: a call that awaits longer than about 45 seconds times out. Wait with `computer` `wait` (10 s at most per call) and check the asset list.
- Never patch the page's `URL.createObjectURL` or `HTMLAnchorElement.prototype.click` (the old `tools/buildings/tripo_export.js` did): it silently swallows Chrome's downloads.

## Uploading images

Convert the reference to PNG first (`dwebp -quiet in.webp -o out.png`), into the scratchpad. Then:
1. In the left panel (Model → HD Model), the first of the four input icons is **single image**; the second is multiview (Front, Left, Right, Back).
2. `find` "image file upload input" gives the hidden `<input type=file>`'s ref; `file_upload` with the PNG's absolute path fills it (under 10 MB per call). Never click the input: it opens a native picker you can't drive.
3. To replace an image, click the bin icon on its thumbnail first, then `find` the input again (refs change).

## Settings that keep the concept

Learned on the ants' Soldier and Door-head (2026-10-29), whose first takes Jiri called horrible:
- **One front image, single-image mode.** Don't feed painted side or back views: a painted side invents its own body (a long neck, a thin torso), and Tripo trusts it over the front. Tripo infers the depth from the front well.
- **Generate at full detail**, then reduce. Geometry & Texture: Topology **Quad**, Polycount at the top of the slider (drag it right; about 45,000), Texture on, 2K, Remove Lighting and PBR on, H3.1 Best Quality. Asking for a game budget at generation (1,800 quads) cuts the spikes, mandibles and armour volume away.
- Click **Generate** through its ref (`find` "Generate button at bottom of left panel"): a coordinate click right after an upload, or while the settings panel is open, is often swallowed. Confirm a new "Generating…" tile appears top left of the asset list.
- The settings are saved between generations; open Geometry & Texture and check Quad and the polycount before every Generate anyway.
- Typing in the polycount box works (triple-click, type, Tab); the slider drag is the reliable way to the maximum.
- **Porous or lattice shapes** (a nest cast, coral, bone with many holes): a Quad generation stalls for 15 minutes and fails (credits come back), with or without Ultra Mesh Quality. Generate with Topology **Triangle** instead (about 50,000, done in 2–3 minutes), then Retopo it to quads. Smart Mesh overshoots on such shapes: 4,800 asked gave 6,325 quads, so ask about three quarters of the target.

## Retopo to the budget

Triangle budgets (Jiri, 2026-10-27): **unit 5k, hero 10k, building 10k, equipment and props 5k** (most far less). A quad is two triangles.
1. Open the full-detail model, then **Retopo** in the left toolbar (`/workspace/retopology/<id>`).
2. Quad, Smart Mesh on, Polygon Count (500–10,000; it resets to 10,000, so set it every time) about **2,000 for a unit** (it came out at 2,239 quads, 4.5k triangles), **4,000 for a hero or building**, less for small pieces. Retopology costs 40.
3. The result replaces the model in its asset tile (and is selected); the viewport's top right shows Topology, Faces (quads) and Vertices. Equipment: about 600 for a sword, 500 for a shield. Check the faces against the budget and the shape against the full-detail model.

Never decimate in Blender: it wrecks the quads.

## Segment (buildings)

On the finished model, **Segment** in the left toolbar (`/workspace/segmentation/<id>`): Balanced (8–15 parts, 40 credits), then Save. Units don't need it.

## Rig and animate (units)

Rig the retopologised model, not the full-detail one.
1. **Rig** in the left toolbar (`/workspace/rigging/<id>`): Humanoid with the Mixamo skeleton for a biped (a creature: Other), **Auto Rig** (20 credits, about 20 seconds). The skeleton shows over the model when it's done.
2. **Animate** (the toolbar entry below Rig, `/workspace/animate/<id>`), tab Preset. For each clip: click the search box, type the preset's name, click its card (it plays and gets a tick). Wait about 6 seconds between cards. A humanoid unit takes `idle`, `walk`, `run`, its attack (`slash` for a blade, `box_01` for a punch or shield bash, `shoot` for a ranged unit), `hit_to_body_01`, `fall` (death) and `cast_a_spell`. The search for `run` also finds `run_upstairs`: `run` is the second card. The clock icon beside the search lists the picked presets.
3. Export (below) with **Number of Animations** opened and **Select All** ticked (it starts at 0, which exports a bare skeleton). Leave "Animation stay in Place" off: `tools/units/tripo_unit.py` takes the root motion out itself. The GLB's clips are named `<preset>.001`.

## Export

1. Select the model in the asset list, then **Export** at the bottom of the viewport (`find` "Export button in bottom toolbar" gives its ref; its position moves between pages).
2. In the dialog: Format **GLB** (it remembers the last format; FBX is the default the first time), texture 2K, then the yellow **Export**.
3. Chrome downloads it to `~/Downloads` under Tripo's own title ("fantasy armored demon 3d model.glb"; typing a file name into the dialog doesn't stick). Wait for the newest `*.glb` there, then move it straight into the scratchpad under your own name:
   ```bash
   until f=$(ls -t ~/Downloads | head -1); [[ $f == *.glb ]]; do sleep 3; done
   mv ~/Downloads/"$f" <scratchpad>/models/<race>-<name>.glb
   ```
   Leave nothing of yours in Downloads, and never commit a raw export.

## Check before showing Jiri

- Look at the model in Tripo from the front, the side and from above at the game angle (drag the viewport), next to the agreed concept. The concept is the bar: if the silhouette, proportions or signature drifted, find which input caused it before generating again, and don't show it as a result.
- If the input image itself drifted from the concept (a turnaround painted thinner or taller than the sheet), repaint the input first; Tripo reproduces its input faithfully.
- One Tripo account is shared between sessions: touch only your own models.
