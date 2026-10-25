/**
 * Sets the queues' map pool (docs/server.md#queues-and-matchmaking): exactly the maps named,
 * with their names and previews read from the game's content, the way the seed reads them.
 *
 *   npm run ladder-pool -- frozen-dream sc2-oxide … [--base ../base]
 *
 * Each id is a map package `base/maps/**\/<id>.utcm`; the content is only read.
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { LadderDb, type MapRow } from "./db";
import { ladderPaths } from "./paths";

const { values, positionals } = parseArgs({ allowPositionals: true, options: { base: { type: "string" } } });
const here = fileURLToPath(new URL(".", import.meta.url));
const base = resolve(values.base ?? join(here, "../../../base"));
const paths = ladderPaths();

/** Every map package under `base/maps`, by its folder's id. */
function packages(): Map<string, string> {
  const out = new Map<string, string>();
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const p = join(dir, entry.name);
      if (entry.name.endsWith(".utcm")) out.set(entry.name.slice(0, -".utcm".length), p);
      else walk(p);
    }
  };
  walk(join(base, "maps"));
  return out;
}

function main(): void {
  const ids = [...new Set(positionals.map((s) => s.trim()).filter(Boolean))];
  if (ids.length === 0) {
    console.error("Usage: npm run ladder-pool -- <map id> [<map id> …] [--base ../base]");
    process.exit(1);
  }
  const found = packages();
  const missing = ids.filter((id) => !found.has(id));
  if (missing.length) {
    console.error(`No map package under ${join(base, "maps")} for: ${missing.join(", ")}`);
    process.exit(1);
  }
  const maps: (MapRow & { source: string | null })[] = ids.map((id) => {
    const dir = found.get(id)!;
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const preview = join(dir, "preview.webp");
    return { id, name: String(pkg.name ?? pkg.id ?? id), preview: null, source: existsSync(preview) ? preview : null };
  });

  mkdirSync(dirname(paths.db), { recursive: true });
  const db = new LadderDb(paths.db);
  db.transaction(() => {
    for (const map of maps) {
      let preview: string | null = null;
      if (map.source) {
        preview = `maps/${map.id}.webp`;
        mkdirSync(join(paths.media, "maps"), { recursive: true });
        copyFileSync(map.source, join(paths.media, preview));
      }
      db.putMap({ id: map.id, name: map.name, preview });
    }
    db.setPool(ids);
  });
  console.log(`${paths.db}: the queues' pool is ${db.pool().map((m) => `${m.name} (${m.id})`).join(", ")}`);
  db.close();
}

main();
