// Dumps what the old TypeScript loader makes of the data, for comparison with the Rust
// reader (`just parity`). Run with the old repository's tsx:
//   <old>/node_modules/.bin/tsx tools/parity/dump-ts.mts <old repository> <output.json>
import {writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [oldRepo, output] = process.argv.slice(2);
if (!oldRepo || !output) throw Error('usage: dump-ts.mts <old repository> <output.json>');
const load = (file: string) => import(join(oldRepo, file));

const {content} = await load('src/content/builtin.ts');
const {coreEffects} = await load('src/content/effects/library.ts');

writeFileSync(output, JSON.stringify({
  definitions: content.definitions,
  rules: content.rules,
  actions: content.actions,
  renderAssets: content.assets,
  abilities: content.abilityLibrary,
  effects: coreEffects,
}));
