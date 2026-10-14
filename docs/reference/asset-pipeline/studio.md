# Asset Studio

Asset Studio is the local image workshop and published-asset library for **Under the Canopy**. Start it with `npm run dev:tools`, then open **http://127.0.0.1:5175/**. The game stays on port 5173; the wiki uses its own server.

## Create an icon

1. Open **Provider settings**, paste an OpenAI API key, and save it locally. Saving does not make a paid request. The presence indicator never returns the saved key.
2. Choose **Create asset**. Give it a name such as *Heart of the Forest*, choose the category and check the filename slug.
3. Describe the subject. Choose a style collection and inspect its selected references. Click references to add/remove them; assign **style**, **subject** or **layout** roles. Only selected images are sent to OpenAI.
4. Choose the API quality, background and candidate count. The default generation size is 1024×1024. **The game export is 128×128**, not the generation request size.
5. Generate. The server owns the job, so closing the browser does not cancel it. The job list shows its actual state without fabricated progress percentages.
6. Open the completed job. Inspect the final export at 128px and 64px. Use **Reprocess originals** to change containment/center cropping or enter a crop rectangle in original-image pixels. Processing always starts from retained original bytes. No stretching or silent upscaling.
7. **Approve this export**, then **Publish to game**. Approval is attached to the exact source/output hashes, request and target revision. Reframing clears it.
8. The asset appears in the library. An icon can be assigned to an existing unit, building or item using **Assign to a game definition**. Replacing an existing asset keeps its render IDs, so all consumers receive the same replacement after a deliberate game reload.

**Import your own image** takes PNG, JPEG or WebP through the same decoding, framing, validation, approval and publication steps, without making an API call.

## Upload style or interface references

In **Create asset → Upload reference images**, choose one or several PNG, JPEG or WebP files (20 MiB and 40 megapixels maximum each). Thumbnails appear immediately after validation; uploaded images are selected automatically. They are stored locally for reuse and do not become published game assets. Uploading does not call OpenAI.

Use **Style** for materials and painting, **Layout** for the arrangement/proportions of an interface screenshot, or **Subject** for the object being depicted. Click a thumbnail or its remove button to deselect it. Library references and uploaded references share the limit of 16 selected images. Their numbered order is the order sent to the provider; an edit mask must match reference 1.

For a transparent HUD component, select **Transparent interface rim**. This selects the woodland interface style, transparent PNG output, and interface export dimensions; your uploaded references stay selected. Describe which rim to extract/recreate, choose the output dimensions and set the protected opening rectangle under **Advanced API controls**. For example: “Use this interface screenshot as reference. Recreate only the central selection-panel rim. Remove all portraits, text, bars, icons and background. Transparent inside and outside.” Review actual alpha before publishing.

Click **Generate** to send the selected references to OpenAI. Jobs retain exact reference bytes and hashes; published revisions retain those copies too. The job review shows the reference thumbnails used. Closing and reopening Studio keeps the uploaded-reference shelf available.

CLI: `npm run assets:studio -- reference tmp/reference.json`, where the JSON is `{ "name": "Interface screenshot", "file": "/absolute/path/interface.png" }`. Use the returned ID in a job reference: `{ "source": "upload", "id": "…", "role": "layout" }`. Existing library references can omit `source` or use `"library"`.

## API controls

The implemented adapter uses `gpt-image-2`, with the selectable `gpt-image-2-2026-04-21` snapshot. It exposes quality, generation dimensions, background, PNG/JPEG/WebP, applicable compression, candidate count, moderation, streaming and partial-image count. References use the edits endpoint; a request without references uses generations. An optional PNG edit mask must match reference 1's dimensions and include alpha.

There is no invented seed, sampler or style-strength control. Reference roles are instructions in the assembled prompt, not a claimed API parameter. The job inspector shows the assembled prompt, parameters, reference hashes and returned usage. Model capabilities were checked against the [image generation](https://developers.openai.com/api/reference/resources/images/methods/generate) and [image editing](https://developers.openai.com/api/reference/resources/images/methods/edit) API documentation.

A failed or interrupted request is not silently resubmitted. An ambiguous result is **unknown**; a new generation requires a new user submission. Canceling an in-flight request stops local waiting; it does not promise that the provider stopped or will not bill it. A local submission ID deduplicates repeated delivery of that same request.

## Interface images

Four checked JSON profiles live in `art/profiles/images.json`:

- **Icon:** 128×128 sRGB PNG; 64 KiB warning budget; optional alpha.
- **Interface rim:** component-specific size; alpha required across both the declared protected opening and the exterior pixel edge. The current opening tool is a normalized rectangle, not an arbitrary polygon editor.
- **Interface fill:** separate textured HP/MP fill. The reviewer shows it clipped at 100%, 75%, 50%, 25% and 0% without squeezing the texture.
- **Interface image:** general image with a declared export size.

The reviewer supports checkerboard, white and near-black backdrops. Validation examines decoded alpha. A painted checkerboard or a transparent corner cannot qualify a solid-center image as a rim.

Mechanical validation is not an aesthetic verdict. Check silhouette, legibility, material treatment and framing before approval. The current validator does not detect baked text or evaluate visual quality automatically.

## Storage and publication

The canonical [publication contract](publication.md) defines working versus released state. Definitions and role-named resources live in `art/assets/<id>/`; released resources live in `assets/library/<id>/` and snapshots in `assets/authoring/published.json`. Each canonical asset folder keeps one master (an editable Blender source or original image) next to its definition. Profiles and curated style references live in `art/profiles/` and `art/styles/`.

Use `npm run assets:compile` to validate released bytes and regenerate deterministic indexes. It does not publish working drafts. Reload a game/editor page deliberately after publication; do not replace an in-progress map document through asset hot reload.

`.asset-work/` contains ignored local jobs, credentials and recovery journals. Do not remove it as documentation clutter. Keys stay on the server and never enter jobs, manifests or browser bundles. Temporary captures and comparison renders belong in ignored `tmp/`.

## Recovery and concurrency

The local service runs one provider request at a time and serializes review/publication mutations. CLI commands use that same running server, rather than starting another writer.

Publication persists canonical source/provenance, verifies current revision and exact approval, checks all existing output hashes, then journals the destination files. Complete temporary files are renamed into place; the manifest is the final logical commit. On interruption, pending journals roll back and completed publication is reconciled to its job. Studio reads wait behind publication; the game dev server gates asset requests during the short commit window and suppresses asset hot reload. Reload after publication to pick up the revision.

This is local authoring tooling. Do not expose the Vite service publicly. It binds loopback and requires a same-origin session token for writes. It is not a multi-user production asset server.

## Agent and terminal workflow

Run the Studio server first. Then:

```sh
npm run assets:studio -- library
npm run assets:studio -- jobs
npm run assets:studio -- create tmp/request.json
npm run assets:studio -- approve tmp/approval.json
npm run assets:studio -- publish tmp/approval.json
npm run assets:studio -- targets
npm run assets:studio -- assign tmp/assignment.json
```

`create` accepts `{ "request": ... }` using `jobRequestSchema` in `tooling/asset-studio/shared/schema.ts`. Add `importFile` for a local manual import. `approve` takes `{ "job", "candidate", "outputHash" }`; `publish` takes `{ "job", "candidate" }`. `process` takes `{ "job", "candidate", "transform" }`; `cancel` takes `{ "job" }`. `assign` takes `{ "asset", "definition", "revision" }`, with the current definition revision from `targets`.

Do not put API keys in request files, command arguments, prompts or chat. Use Provider settings or the server environment.

## Verification

Run the Studio tests for decoding, export profiles, path containment, revision checks and publication recovery. Check actual exported pixels and model previews in the tools/game renderer. A successful API response is not proof of visual quality. Do not store historical pass counts as a current guarantee.

The Studio and game Vite servers use separate dependency caches. Meshy generation, arbitrary protected-opening polygons and automatic aesthetic scoring are not implemented here.

## Generated woodland HUD

The HUD's new woodland textures were generated using Asset Studio with the uploaded approved interface screenshot. Each `art/assets/image.woodland-*` package keeps its original `source.png` and published `image.png`. No artwork was copied directly into the runtime outside the Studio publication transaction.

The runtime now uses `woodland-connected-hud.png`, generated from the approved connected concept through Asset Studio. Its exterior has real alpha; its minimap opening, uninterrupted center and twelve command cells retain matching dark textured interiors. The minimap is drawn into its opening at runtime. Separate square rims remain for inventory and production queue items, with a horizontal frame for resources/vitals/learning, two textured vital fills, and subdued leather behind four inventory slots.

The dock preserves the master image's 3:1 aspect ratio. Minimap, selection and command hit targets use normalized coordinates in its 2160 × 720 canvas, so all three rows scale with their painted cells. The compact setting caps the connected dock at 1240px; spread uses the screen width. The shared settings menu also exposes a persistent 30–100% HUD size slider (default 65%). It uniformly scales the entire dock around its bottom-center anchor, including ornaments, text and hit targets; it does not change 3D render resolution. Transparent bottom padding is offset below the viewport. The existing live 3D portrait still uses the same renderer and scissor pass: a matching cutout in the HUD layer reveals it, and its scene background samples the corresponding region of the HUD texture to avoid a separate portrait rectangle. Keep the cutout coordinates, portrait bounds, and texture UV crop synchronized when changing this layout. Buildings use armor/HP and the active production meter plus waiting queue. Hero abilities retain their declarative fixed slots; the learning banner sits below the twelve cells without shifting them.

Vital textures are clipped by the live percentage rather than resized, preserving their spatial detail as the pool changes. HP uses the existing green/yellow/orange/red thresholds. Text stays above the fill and frame. Commands use the connected artwork’s grid without duplicate rims or placeholder elements. Inventory cells retain independent rims. The resource strip is centered at the top of the screen, independent of dock scale. Its tooltips open below the strip; command tooltips sit 20px above the command grid. The minimap has no map-name caption. Experience fills clockwise from twelve o’clock around the hero’s level badge.

Asset Studio now supports transparent export padding and revising export dimensions/protected opening during review. All refinements reprocess the original, invalidate approval, and preserve strict alpha validation. Reference previews preserve their aspect ratio, including long HUD strips.
