// Dumps what the old TypeScript game makes of the published asset data, for comparison with
// the Rust reader (`just parity-assets`). Run with the old repository's tsx:
//   <old>/node_modules/.bin/tsx tools/parity/assets-ts.mts <old repository> <output.json>
//
// The manifest and landscape catalogue come from the game modules that read them. The
// other files are cast or imported as-is by modules that need Vite (`import.meta.glob`,
// `?url`) or three.js, so their JSON is read directly: the cast is the value.
import {readFileSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [oldRepo, output] = process.argv.slice(2);
if (!oldRepo || !output) throw Error('usage: assets-ts.mts <old repository> <output.json>');
const load = (file: string) => import(join(oldRepo, file));
const json = (file: string) => ({file, value: JSON.parse(readFileSync(join(oldRepo, file), 'utf8'))});

const manifest = await load('src/shared/assets/manifest.ts');
const {landscapeAssets} = await load('src/shared/authoring/project.ts');

writeFileSync(output, JSON.stringify({
  // src/shared/assets/manifest.ts
  manifest: {version: json('assets/manifest.json').value.version, records: manifest.publishedAssets},
  renderAssets: manifest.renderAssets,
  scenery: manifest.sceneryCatalogue.assets,
  // src/shared/authoring/project.ts, groundTextures.ts
  landscape: landscapeAssets,
  // src/shared/assets/models.ts
  models: json('assets/authoring/models.json').value,
  // src/shared/map/library.ts
  mapPreviews: json('assets/maps/previews/index.json').value,
  // src/render/atmosphere/daytimeLuts.ts
  daytimeGrades: json('assets/library/asset.texture.woodland-daytime-grades/data.json'),
  // src/render/terrain/sourceReflection.ts
  reflectionCube: json('assets/library/asset.texture.woodland-reflection/data_2.json'),
  brdfTable: json('assets/library/asset.texture.woodland-brdf/data_2.json'),
}));
