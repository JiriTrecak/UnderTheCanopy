# Interface icons

Commands, portraits, armor types and item costs resolve stable image asset IDs through the content registry and published manifest. Runtime icons are 128 × 128 PNGs in canonical `assets/library/<id>/` packages. Do not bake text, shortcuts or UI frames into them; the HUD owns those elements.

Full-resolution masters and generation receipts live in canonical `art/assets/<id>/` source and generation roles. New revisions use the [Asset Studio workflow](../asset-pipeline/studio.md), which retains source and provenance in canonical asset roles. The full-resolution images are source inputs; the small runtime derivatives are not substitutes for them.

Review at 128px and 64px. Keep one readable subject, strong silhouette and the established painted material treatment. Amber depicts forest resin. Definitions own icon references, while command priority, categories and availability come from gameplay declarations.
