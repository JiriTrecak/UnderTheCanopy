// Dumps what the old TypeScript parser makes of every shipped map, for comparison with the
// Rust reader (`just parity-maps`). Run with the old repository's tsx:
//   <old>/node_modules/.bin/tsx tools/parity/maps-ts.mts <old repository> <output.json>
// For each map (found as src/shared/map/library.ts finds them) it records the parsed
// document or the parse error, and the verdict of `playableMapError`.
import {readFileSync, readdirSync, statSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';

const [oldRepo, output] = process.argv.slice(2);
if (!oldRepo || !output) throw Error('usage: maps-ts.mts <old repository> <output.json>');
const load = (file: string) => import(join(oldRepo, file));

const {readUtcMap} = await load('src/shared/map/utcmap.ts');
const {playableMapError} = await load('src/shared/map/playable.ts');

function files(folder: string, relative: string): string[] {
  let names: string[];
  try { names = readdirSync(folder); } catch { return []; }
  return names.filter(name => !name.startsWith('.')).flatMap(name => {
    const path = join(folder, name), file = `${relative}/${name}`;
    if (statSync(path).isDirectory()) return files(path, file);
    return name.endsWith('.utcmap') ? [file] : [];
  });
}

const maps: Record<string, unknown> = {};
for (const group of ['campaign', 'skirmish', 'showcase']) {
  for (const file of files(join(oldRepo, 'assets/maps', group), `assets/maps/${group}`)) {
    const id = file.split('/').pop()!.replace('.utcmap', '').toLowerCase();
    const started = performance.now();
    const read = readUtcMap(JSON.parse(readFileSync(join(oldRepo, file), 'utf8')));
    const parseMs = performance.now() - started;
    maps[id] = 'map' in read
      ? {file, parseMs, map: read.map, playable: playableMapError(read.map)}
      : {file, parseMs, error: read.error};
  }
}
writeFileSync(output, JSON.stringify({maps}));
