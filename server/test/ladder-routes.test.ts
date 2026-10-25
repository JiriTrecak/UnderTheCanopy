// The ladder website under /ladder (docs/server.md#ladder): its pages, JSON API, replay
// downloads and files, served from a database the test fills with its own races and players.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, test } from "node:test";
import { LadderDb } from "../src/ladder/db";
import { PLACEMENT_MATCHES, REPLAY_DAYS } from "../src/ladder/leagues";
import { ladderPaths, type LadderPaths } from "../src/ladder/paths";
import { createLadder, type Ladder } from "../src/ladder/routes";

const NOW = new Date("2026-11-20T18:00:00Z");
const DAY = 86400000;
const REPLAY = Buffer.concat([Buffer.from("UTCR"), Buffer.from([1, 0, 0, 0, 7, 7, 7])]);

let server: Server;
let ladder: Ladder;
let base: string;
let paths: LadderPaths;
const ids: Record<string, number> = {};

before(async () => {
  const data = mkdtempSync(join(tmpdir(), "ladder-"));
  paths = ladderPaths({ LADDER_DATA: data });
  mkdirSync(paths.replays, { recursive: true });
  writeFileSync(join(paths.replays, "m1.utcr"), REPLAY);

  const db = new LadderDb(paths.db);
  db.transaction(() => {
    db.putRace({ id: "moths", name: "Moths", description: "Dusty wings.", position: 0, accent: "sky" });
    db.putRace({ id: "snails", name: "Snails", description: "Slow shells.", position: 1, accent: "violet" });
    db.putHero({ id: "hero.moth", race: "moths", name: "Lamp Chaser", portrait: null });
    db.putHero({ id: "hero.snail", race: "snails", name: "Shell <Warden>", portrait: null });
    db.putMap({ id: "test-pond", name: "Test Pond", preview: null });
    const season = db.addSeason({ number: 1, name: "Season 1", startsAt: new Date("2026-10-01T00:00:00Z"), endsAt: new Date("2027-01-01T00:00:00Z") });
    const ace = db.addPlayer({ playerId: "Ace1", displayName: "Ace", createdAt: new Date("2026-09-01T00:00:00Z") });
    const bea = db.addPlayer({ playerId: "Bea_2", displayName: "Bea", createdAt: new Date("2026-09-01T00:00:00Z") });
    const cid = db.addPlayer({ playerId: "Cid", displayName: "Cid <script>", createdAt: new Date("2026-09-01T00:00:00Z") });
    // Ace beats Bea's snails more often than not; Cid plays only twice, still placing.
    const play = (winner: 0 | 1, at: Date, a: number, b: number, replay: string | null = "sample") =>
      db.recordMatch({
        season,
        playedAt: at,
        map: "test-pond",
        duration: 754,
        replay,
        winner,
        sides: [
          { player: a, race: "moths", hero: "hero.moth" },
          { player: b, race: "snails", hero: "hero.snail" },
        ],
      });
    const start = new Date("2026-10-02T20:00:00Z").getTime();
    for (let i = 0; i < 8; i++) play(i % 4 === 3 ? 1 : 0, new Date(start + i * DAY), ace, bea);
    ids.real = play(0, new Date(NOW.getTime() - 2 * DAY), ace, bea, "m1.utcr");
    ids.cid = play(1, new Date(NOW.getTime() - DAY), cid, bea);
    ids.first = 1;
  });
  db.close();

  ladder = createLadder({ paths, now: () => NOW });
  server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://ladder");
    void ladder.handle(req, res, url);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

after(() => {
  server.close();
  ladder.close();
});

const get = (path: string) => fetch(base + path, { redirect: "manual" });
const json = async (path: string): Promise<any> => (await get(path)).json();

test("the overview shows the season, each race's best players and recent matches", async () => {
  const res = await get("/ladder");
  assert.equal(res.status, 200);
  assert.match(res.headers.get("content-type") ?? "", /text\/html/);
  const page = await res.text();
  assert.match(page, /Season 1/);
  assert.match(page, /Moths/);
  assert.match(page, /Snails/);
  assert.match(page, /href="\/ladder\/player\/Ace1"/);
  assert.match(page, /Test Pond/);
  assert.match(page, /\/ladder\/static\/ladder\.css/);
  assert.doesNotMatch(page, /cdn\.tailwindcss|<script/i, "no runtime Tailwind or scripts");
});

test("names from the database are escaped", async () => {
  const page = await (await get("/ladder/player/Cid")).text();
  assert.match(page, /Cid &lt;script&gt;/);
  assert.doesNotMatch(page, /Cid <script>/);
});

test("a race's leaderboard lists placed players best first, filterable by league", async () => {
  const board = await json("/ladder/api/leaderboard/moths");
  assert.equal(board.total, 1, "only Ace has played the placement matches on moths");
  assert.equal(board.rows[0].playerId, "Ace1");
  assert.equal(board.rows[0].games, 9);
  assert.equal(board.rows[0].wins + board.rows[0].losses, 9);
  const snails = await json("/ladder/api/leaderboard/snails");
  assert.equal(snails.rows[0].playerId, "Bea_2");
  const league = snails.rows[0].league;
  const filtered = await json(`/ladder/api/leaderboard/snails?league=${league}`);
  assert.equal(filtered.total, 1);
  const other = await json(`/ladder/api/leaderboard/snails?league=${(league + 1) % 5}`);
  assert.equal(other.total, 0);
  const page = await get("/ladder/race/moths");
  assert.equal(page.status, 200);
  assert.match(await page.text(), /Ace1/);
  assert.equal((await get("/ladder/race/wasps")).status, 404);
  assert.equal((await get("/ladder/api/leaderboard/wasps")).status, 404);
});

test("a player's profile has a rating per race, placement, and recent matches", async () => {
  const cid = await json("/ladder/api/players/cid");
  assert.equal(cid.playerId, "Cid", "Player IDs are found without case");
  assert.equal(cid.ratings.length, 1);
  assert.equal(cid.ratings[0].standing, `Placement 1/${PLACEMENT_MATCHES}`);
  assert.equal(cid.ratings[0].league, null);
  assert.equal(cid.matches.length, 1);
  assert.equal(cid.matches[0].players[0].won, false);
  const ace = await json("/ladder/api/players/Ace1");
  assert.equal(ace.ratings[0].rank, 1);
  assert.equal(ace.matches[0].id, ids.real, "newest first");
  const page = await get("/ladder/player/Ace1");
  assert.equal(page.status, 200);
  assert.match(await page.text(), /Most played hero/);
  assert.equal((await get("/ladder/player/Nobody")).status, 404);
});

test("matches list newest first and link their replays", async () => {
  const list = await json("/ladder/api/matches");
  assert.equal(list.total, 10);
  assert.equal(list.matches[0].id, ids.cid);
  assert.equal(list.matches[0].replay, `/ladder/replays/${ids.cid}`);
  const page = await (await get("/ladder/matches")).text();
  assert.match(page, new RegExp(`/ladder/replays/${ids.real}`));
});

test("a replay downloads as its .utcr file; a seeded one as the labelled sample", async () => {
  const real = await get(`/ladder/replays/${ids.real}`);
  assert.equal(real.status, 200);
  assert.equal(real.headers.get("content-type"), "application/vnd.canopy.replay");
  assert.match(real.headers.get("content-disposition") ?? "", /attachment; filename="Test Pond .* Ace1 vs Bea_2\.utcr"/);
  assert.deepEqual(Buffer.from(await real.arrayBuffer()), REPLAY);
  const sample = await get(`/ladder/replays/${ids.cid}`);
  assert.equal(sample.status, 200);
  assert.match(sample.headers.get("content-type") ?? "", /text\/plain/);
  assert.match(sample.headers.get("content-disposition") ?? "", /SAMPLE/);
  assert.match(await sample.text(), /NOT A REPLAY/);
  assert.equal((await get("/ladder/replays/99999")).status, 404);
});

test(`replays older than ${REPLAY_DAYS} days are gone`, async () => {
  assert.equal((await get(`/ladder/replays/${ids.first}`)).status, 200, "still kept on 20 November");
  // The first match was played on 2 October; ninety days on it has expired.
  const later = new Date(new Date("2026-10-02T20:00:00Z").getTime() + REPLAY_DAYS * DAY + 1000);
  const expired = createLadder({ paths, now: () => later });
  const s = createServer((req, res) => void expired.handle(req, res, new URL(req.url ?? "/", "http://ladder")));
  await new Promise<void>((resolve) => s.listen(0, "127.0.0.1", resolve));
  const gone = await fetch(`http://127.0.0.1:${(s.address() as AddressInfo).port}/ladder/replays/${ids.first}`);
  assert.equal(gone.status, 410);
  s.close();
  expired.close();
});

test("files are served from the site and media folders only", async () => {
  const css = await get("/ladder/static/ladder.css");
  assert.equal(css.status, 200);
  assert.match(css.headers.get("content-type") ?? "", /text\/css/);
  assert.equal((await get("/ladder/static/img/hero.webp")).headers.get("content-type"), "image/webp");
  for (const path of ["/ladder/static/..%2f..%2fladder%2fdb.ts", "/ladder/static/%2e%2e/routes.ts", "/ladder/media/..%2fladder.sqlite", "/ladder/static/sample-replay.exe"]) {
    assert.equal((await get(path)).status, 404, path);
  }
});

test("search finds players and jumps straight to an exact Player ID", async () => {
  const exact = await get("/ladder/search?q=bea_2");
  assert.equal(exact.status, 302);
  assert.equal(exact.headers.get("location"), "/ladder/player/Bea_2");
  const some = await get("/ladder/search?q=e");
  assert.equal(some.status, 200);
  assert.match(await some.text(), /Ace1/);
  const api = await json("/ladder/api/players?q=c");
  assert.deepEqual(api.players.map((p: { playerId: string }) => p.playerId).sort(), ["Ace1", "Cid"]);
});

test("a trailing slash redirects and only reads are allowed", async () => {
  const slash = await get("/ladder/");
  assert.equal(slash.status, 302);
  assert.equal(slash.headers.get("location"), "/ladder");
  assert.equal((await fetch(`${base}/ladder`, { method: "POST" })).status, 405);
  assert.equal((await get("/ladder/nothing-here")).status, 404);
});
