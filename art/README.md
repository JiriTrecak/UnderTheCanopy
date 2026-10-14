# Editable art

- `assets/<asset-id>/`: `asset.json`, its published outputs and one master source (`source.blend`, `source.png`, …).
- `styles/`: approved art direction and image-reference collections.
- `profiles/`: export and validation settings.

Runtime resources live in `assets/library/` at the repository root. Publish through the asset editor or shared authoring service; see [publication](../docs/asset-pipeline/publication.md) and [Asset Studio](../docs/asset-pipeline/studio.md).

Keep one file that represents each asset's current state; do not retain numbered iterations, provider downloads, references, previews or generation receipts. Put temporary comparisons, screenshots and delivery reports in ignored `tmp/`, not a permanent proof archive.
