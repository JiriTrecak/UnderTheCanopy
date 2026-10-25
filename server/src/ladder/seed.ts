/**
 * Fills the ladder database with plausible random data (docs/server.md#ladder): players,
 * a season and its ranked matches, rated through the ladder's own Glicko-2 and leagues.
 * Races, heroes and maps come from the game's content (base/), never named here.
 *
 *   npm run seed-ladder [-- --reset] [--players 320] [--matches 4800] [--seed 7] [--base ../base]
 *     [--maps 5 | --maps frozen-dream,sc2-oxide,…]
 *
 * Seeded matches point their replay at the labelled sample file (no real replays exist yet).
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { INITIAL } from "./glicko";
import { LadderDb } from "./db";
import { seasonOf } from "./leagues";
import { ladderPaths } from "./paths";

/** Accents the site gives races, in the races' order (classes in pages.ts). */
export const ACCENTS = ["rose", "violet", "sky"];

const { values } = parseArgs({
  options: {
    reset: { type: "boolean", default: false },
    players: { type: "string", default: "320" },
    matches: { type: "string", default: "4800" },
    seed: { type: "string", default: "7" },
    base: { type: "string" },
    maps: { type: "string", default: "5" },
  },
});

const here = fileURLToPath(new URL(".", import.meta.url));
const base = resolve(values.base ?? join(here, "../../../base"));
const paths = ladderPaths();
const random = mulberry32(Number(values.seed));

// ---- Content -----------------------------------------------------------------------------

type ContentRace = { id: string; name: string; description: string; heroes: { id: string; name: string; portrait: string | null }[] };

function readJson(file: string): any {
  return JSON.parse(readFileSync(file, "utf8"));
}

function text(): Record<string, string> {
  const dir = join(base, "text/en");
  const out: Record<string, string> = {};
  for (const f of readdirSync(dir)) if (f.endsWith(".json")) Object.assign(out, readJson(join(dir, f)));
  return out;
}

/** Every unit definition file by id. */
function unitFiles(): Map<string, string> {
  const out = new Map<string, string>();
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, entry.name);
      if (entry.isDirectory()) walk(p);
      else if (entry.name.endsWith(".json")) {
        const id = readJson(p).id;
        if (typeof id === "string") out.set(id, p);
      }
    }
  };
  walk(join(base, "units"));
  return out;
}

/** An asset id's image file (`asset.portraits.units.x` → `assets/portraits/units/x/image.png`). */
function assetImage(asset: string | undefined): string | null {
  if (!asset?.startsWith("asset.")) return null;
  const dir = join(base, "assets", ...asset.slice("asset.".length).split("."));
  for (const name of ["image.png", "image.webp", "image.jpg"]) if (existsSync(join(dir, name))) return join(dir, name);
  return null;
}

function races(words: Record<string, string>): ContentRace[] {
  const units = unitFiles();
  const dir = join(base, "races");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".json"))
    .sort()
    .map((f) => {
      const race = readJson(join(dir, f));
      const choices: string[] = race.startingSetup?.hero?.choices ?? [];
      return {
        id: race.id,
        name: words[`race.${race.id}.name`] ?? race.id,
        description: words[`race.${race.id}.description`] ?? "",
        heroes: choices.map((id) => {
          const file = units.get(id);
          const def = file ? readJson(file) : {};
          return { id, name: words[`${id}.name`] ?? id, portrait: assetImage(def.icon) };
        }),
      };
    });
}

/** The season's map pool: the maps named in `--maps` (ids, comma-separated), or that many
 * two-player skirmish maps picked at random. */
function mapPool(choice: string): { id: string; name: string; preview: string | null }[] {
  const named = /^\d+$/.test(choice) ? null : choice.split(",").map((s) => s.trim()).filter(Boolean);
  const count = named ? named.length : Number(choice);
  const dir = join(base, "maps/skirmish");
  const duels = readdirSync(dir)
    .filter((f) => f.endsWith(".utcm"))
    .sort()
    .flatMap((f) => {
      const pkg = readJson(join(dir, f, "package.json"));
      const map = readJson(join(dir, f, "map.json"));
      if (named ? !named.includes(String(pkg.id)) : (map.playerStarts ?? []).length !== 2) return [];
      const preview = join(dir, f, "preview.webp");
      return [{ id: String(pkg.id), name: String(pkg.name ?? pkg.id), preview: existsSync(preview) ? preview : null }];
    });
  if (named && duels.length !== named.length) throw Error(`Unknown maps in --maps ${choice}`);
  return shuffle(duels).slice(0, count).sort((a, b) => a.name.localeCompare(b.name));
}

// ---- Random helpers ----------------------------------------------------------------------

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function normal(): number {
  const u = Math.max(random(), 1e-9);
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * random());
}

function pick<T>(list: readonly T[]): T {
  return list[Math.floor(random() * list.length)]!;
}

function weighted<T>(list: readonly T[], weight: (t: T) => number): T {
  const total = list.reduce((s, t) => s + weight(t), 0);
  let r = random() * total;
  for (const t of list) {
    r -= weight(t);
    if (r <= 0) return t;
  }
  return list[list.length - 1]!;
}

function shuffle<T>(list: T[]): T[] {
  for (let i = list.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [list[i], list[j]] = [list[j]!, list[i]!];
  }
  return list;
}

const FIRST = [
  "Moss", "Thorn", "Amber", "Bark", "Root", "Fern", "Dew", "Spore", "Briar", "Husk", "Frost", "Cinder", "Gloom", "Rook", "Sable",
  "Quill", "Burrow", "Hollow", "Lichen", "Pebble", "Mandible", "Chitin", "Silk", "Nettle", "Shade", "Mire", "Dusk", "Ember", "Slate",
  "Cobalt", "Iron", "Brass", "Coil", "Rivet", "Piston", "Lantern", "Twig", "Acorn", "Hazel", "Wisp", "Raven", "Moth", "Drone",
];
const SECOND = [
  "back", "wick", "fang", "claw", "shell", "horn", "leaf", "step", "crawler", "biter", "runner", "warden", "smith", "keeper",
  "breaker", "weaver", "digger", "rider", "spark", "watch", "scout", "jaw", "sting", "drift", "fall", "born", "heart", "brood",
];
const HANDLES = ["Kaya", "Tomas", "Lin", "Ondra", "Mira", "Yusuf", "Petra", "Jun", "Ilse", "Ravi", "Noor", "Eli", "Saoirse", "Kai", "Vera"];

function playerName(taken: Set<string>): { playerId: string; displayName: string } {
  for (;;) {
    const roll = random();
    let display: string;
    if (roll < 0.15) display = pick(HANDLES);
    else if (roll < 0.25) display = `${pick(FIRST)}_${pick(SECOND)}`;
    else display = pick(FIRST) + pick(SECOND);
    const suffix = random() < 0.55 ? String(Math.floor(random() * (random() < 0.5 ? 10 : 1000))) : "";
    const playerId = (display + suffix).slice(0, 32);
    if (taken.has(playerId.toLowerCase())) continue;
    taken.add(playerId.toLowerCase());
    return { playerId, displayName: display.replace(/_/g, " ") };
  }
}

// ---- The season --------------------------------------------------------------------------

type Sim = {
  id: number;
  activity: number;
  skill: Record<string, number>;
  racePref: Record<string, number>;
  heroPref: Record<string, number>;
  /** The queue's view of each race rating (the database holds the truth). */
  mmr: Record<string, number>;
};

function main(): void {
  mkdirSync(paths.data, { recursive: true });
  mkdirSync(join(paths.media, "heroes"), { recursive: true });
  mkdirSync(join(paths.media, "maps"), { recursive: true });
  const db = new LadderDb(paths.db);
  const existing = Number((db.db.prepare("select count(*) n from players").get() as { n: number }).n);
  if (existing > 0 && !values.reset) {
    console.error(`${paths.db} already holds ${existing} players. Pass --reset to replace everything in it.`);
    process.exit(1);
  }

  const words = text();
  const content = races(words);
  if (content.length === 0) throw Error(`No races under ${base}`);
  const pool = mapPool(values.maps!);
  const now = new Date();
  const season = seasonOf(now);
  const heroes = content.flatMap((r) => r.heroes.map((h) => ({ ...h, race: r.id })));

  db.transaction(() => {
    db.clear();
    content.forEach((race, i) => {
      db.putRace({ id: race.id, name: race.name, description: race.description, position: i, accent: ACCENTS[i % ACCENTS.length]! });
      for (const hero of race.heroes) {
        let portrait: string | null = null;
        if (hero.portrait) {
          portrait = `heroes/${hero.id.replace(/[^A-Za-z0-9.-]/g, "-")}.png`;
          copyFileSync(hero.portrait, join(paths.media, portrait));
        }
        db.putHero({ id: hero.id, race: race.id, name: hero.name, portrait });
      }
    });
    for (const map of pool) {
      let preview: string | null = null;
      if (map.preview) {
        preview = `maps/${map.id}.webp`;
        copyFileSync(map.preview, join(paths.media, preview));
      }
      db.putMap({ id: map.id, name: map.name, preview });
    }
    // The seeded maps are the queues' pool.
    db.setPool(pool.map((m) => m.id));
    const seasonId = db.addSeason({ number: season.number, name: `Season ${season.number}`, startsAt: season.starts, endsAt: season.ends });

    // Players: a skill per race, a favourite race and hero, and how much they play.
    const taken = new Set<string>();
    const sims: Sim[] = [];
    const count = Number(values.players);
    for (let i = 0; i < count; i++) {
      const { playerId, displayName } = playerName(taken);
      const id = db.addPlayer({ playerId, displayName, createdAt: new Date(season.starts.getTime() - random() * 200 * 86400000) });
      const talent = 1500 + normal() * 260;
      const main = pick(content).id;
      const skill: Record<string, number> = {};
      const racePref: Record<string, number> = {};
      for (const r of content) {
        skill[r.id] = r.id === main ? talent : talent - 60 - Math.abs(normal()) * 150;
        racePref[r.id] = r.id === main ? 1 : random() < 0.3 ? 0.4 : 0.06;
      }
      const heroPref: Record<string, number> = {};
      for (const h of heroes) heroPref[h.id] = 0.2 + random() ** 2 * 2;
      sims.push({ id, activity: 0.2 + Math.exp(normal() * 0.9), skill, racePref, heroPref, mmr: {} });
    }
    const mmr = (s: Sim, race: string) => s.mmr[race] ?? INITIAL.rating;

    // Matches, oldest first; the queue pairs players of close ratings.
    const total = Number(values.matches);
    const span = now.getTime() - season.starts.getTime();
    const times = Array.from({ length: total }, () => {
      // Busier in the evening (UTC, the London region): hours far from 20:00 are often skipped.
      for (;;) {
        const t = season.starts.getTime() + random() * span;
        const hour = new Date(t).getUTCHours();
        const away = Math.min(Math.abs(hour - 20), 24 - Math.abs(hour - 20));
        if (random() < Math.exp(-(away * away) / 40)) return t;
      }
    }).sort((a, b) => a - b);

    const after = db.db.prepare("select slot, rating_after from match_players where match = ? order by slot");
    for (const time of times) {
      const a = weighted(sims, (s) => s.activity);
      const raceA = weighted(content, (r) => a.racePref[r.id]!).id;
      // Matchmaking: the closest of a dozen queued players.
      const candidates = Array.from({ length: 12 }, () => weighted(sims, (s) => s.activity)).filter((s) => s !== a);
      if (candidates.length === 0) continue;
      const options = candidates.map((s) => ({ s, race: weighted(content, (r) => s.racePref[r.id]!).id }));
      const gap = (o: { s: Sim; race: string }) => Math.abs(mmr(o.s, o.race) - mmr(a, raceA));
      const { s: b, race: raceB } = options.reduce((x, y) => (gap(y) < gap(x) ? y : x));
      const heroOf = (s: Sim, race: string) => {
        const list = heroes.filter((h) => h.race === race);
        return list.length ? weighted(list, (h) => s.heroPref[h.id]!).id : null;
      };
      const chanceA = 1 / (1 + 10 ** ((b.skill[raceB]! - a.skill[raceA]!) / 400));
      const id = db.recordMatch({
        season: seasonId,
        playedAt: new Date(time),
        map: pick(pool).id,
        duration: Math.round(Math.min(3000, Math.max(240, Math.exp(Math.log(840) + normal() * 0.38)))),
        replay: "sample",
        winner: random() < chanceA ? 0 : 1,
        sides: [
          { player: a.id, race: raceA, hero: heroOf(a, raceA) },
          { player: b.id, race: raceB, hero: heroOf(b, raceB) },
        ],
      });
      const [ra, rb] = after.all(id) as { rating_after: number }[];
      a.mmr[raceA] = ra!.rating_after;
      b.mmr[raceB] = rb!.rating_after;
    }
  });

  const stats = db.stats(db.season(now)!.id, now);
  console.log(`Seeded ${paths.db}: season ${season.number}, ${stats.players} players (${stats.placed} placed), ${stats.matches} matches, maps: ${pool.map((m) => m.name).join(", ")}`);
  db.close();
}

main();
