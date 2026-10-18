/**
 * Generates building concept images (gpt-image-2) through the old Asset Studio's own provider
 * and credential store: the key never leaves them, never print it. Run through concept.sh:
 *   tools/buildings/concept.sh <request.json>
 * request.json: {slug, size, n, quality, references: [paths inside the old repository], prompt}
 * Images land in the old repository's `.asset-work/concepts/buildings/` (ignored there).
 */
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import path from 'node:path';

const root = process.cwd(); // the old repository (concept.sh changes into it)
const studio = path.join(root, 'tooling/asset-studio/server');
const {Credentials} = await import(path.join(studio, 'credentials.ts'));
const {openAIProvider} = await import(path.join(studio, 'provider.ts'));

const request = JSON.parse(await readFile(process.argv[2], 'utf8'));
const key = await new Credentials(root).key();
if (!key) throw Error('No key in the Asset Studio credential store.');
const job = {
  prompt: request.prompt,
  references: (request.references ?? []).map((p: string) => ({path: p})),
  mask: request.mask ? {path: request.mask} : undefined,
  request: {parameters: {model: 'gpt-image-2', size: request.size, quality: request.quality ?? 'high', background: request.background ?? 'opaque', output_format: 'png', n: request.n ?? 1, moderation: 'auto', stream: false}},
} as never;
const started = Date.now();
const result = await openAIProvider(root)(job, key, new AbortController().signal, async () => {});
const out = path.join(root, '.asset-work/concepts/buildings');
await mkdir(out, {recursive: true});
const files: string[] = [];
for (const [i, image] of result.images.entries()) {
  const file = path.join(out, `${request.slug}-${i + 1}.png`);
  await writeFile(file, image);
  files.push(file);
}
await writeFile(path.join(out, `${request.slug}.prompt.json`), JSON.stringify({...request, requestId: result.requestId, usage: result.usage}, null, 2));
console.log(JSON.stringify({files, seconds: Math.round((Date.now() - started) / 1000)}, null, 2));
