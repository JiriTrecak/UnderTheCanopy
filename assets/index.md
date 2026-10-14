# Published game assets

`manifest.json` and generated URL bindings resolve the published asset library. Released definitions live in `authoring/published.json`; binaries and data profiles live in `library/<asset-id>/`. Map source lives in `maps/`.

Working definitions and editable resources live in `art/assets/` outside the runtime build. Use [asset publication](../docs/asset-pipeline/publication.md); do not manually replace released bytes without updating their validated hashes and revision.
