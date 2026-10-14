# Woodland HUD

The HUD uses a connected woodland frame with a minimap, selection/portrait region and twelve command cells. Inventory, queue, resource and vital frames remain separate. Artwork is published through canonical asset packages; text and interaction targets remain live HTML controls.

Settings control Compact/Spread layout and HUD scale independently of the 3D resolution. Hit targets use normalized artwork coordinates, so changing the frame requires checking portrait cutouts, minimap bounds, command cells and tooltips together. Vital textures are clipped by percentage rather than squeezed.

`src/ui/settlement/settlementHud.ts` consumes observed state and presentation models. `src/render/portrait/selectionPortrait.ts` renders the selected model through the existing WebGL renderer with its own lighting and instance state. It must not introduce another context or expose hidden simulation data. Inventory size, recruitment capacity and command availability come from gameplay declarations.

See [Asset Studio](asset-pipeline/studio.md) for image authoring and [RTS controls](declarations/rts-controls.md) for player interaction. Keep temporary UI comparisons in ignored `tmp/`.
