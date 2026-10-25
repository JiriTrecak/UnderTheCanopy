// The 1v1 queues (docs/server.md#queues-and-matchmaking): the widening window, pairing,
// vetoes and Random races, stale tickets, the matched room starting by itself or cancelled,
// ranked results and their agreement rules, and the routes. A ladder of the test's own
// races and maps, on a fake clock, timers and random source.
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { MatchHost, type HostedMatch } from "../src/host";
import { LadderDb } from "../src/ladder/db";
import type { ClientMsg, ServerMsg } from "../src/protocol";
import { Queue, accepts, queueRoute, searchRange } from "../src/queue";

const SEASON_START = new Date("2026-10-01T00:00:00Z");
const NOW = new Date("2026-11-20T18:00:00Z");
const content = { type: "ready", content: { abi: "1", sha256: "a".repeat(64) }, build: "b" } as const;

function ladder(withSeason = true) {
  const db = new LadderDb(":memory:");
  db.putRace({ id: "moths", name: "Moths", position: 0, accent: "sky" });
  db.putRace({ id: "snails", name: "Snails", position: 1, accent: "violet" });
  db.putMap({ id: "pond", name: "Pond", preview: null });
  db.putMap({ id: "marsh", name: "Marsh", preview: null });
  db.putMap({ id: "dune", name: "Dune", preview: null });
  db.putMap({ id: "cave", name: "Cave", preview: null });
  db.setPool(["pond", "marsh", "dune"]);
  const season = withSeason ? db.addSeason({ number: 1, name: "Season 1", startsAt: SEASON_START, endsAt: new Date("2027-01-01T00:00:00Z") }) : 0;
  return { db, season };
}

/** A queue over a fresh ladder; `wait` advances a second at a time, polling `keep` tickets. */
function setup(opts: { withSeason?: boolean } = {}) {
  const { db, season } = ladder(opts.withSeason ?? true);
  const clock = { t: 0 };
  const timers: { at: number; fn: () => void; live: boolean }[] = [];
  const schedule = (fn: () => void, ms: number) => {
    const timer = { at: clock.t + ms, fn, live: true };
    timers.push(timer);
    return () => { timer.live = false; };
  };
  const host = new MatchHost(() => clock.t, { schedule, log: () => {} });
  const rolls: number[] = [];
  const logs: string[] = [];
  let next = 0;
  const queue = new Queue({
    db,
    host,
    now: () => clock.t,
    date: () => new Date(NOW.getTime() + clock.t),
    random: () => rolls.shift() ?? 0,
    ticketId: () => `t${next++}`,
    log: (line) => logs.push(line),
  });
  const route = (method: string, path: string, body?: unknown) => queueRoute(queue, method, path, body)!;
  const enter = (playerId: string, extra: Record<string, unknown> = {}): string => {
    const reply = route("POST", "/api/queue", { protocol: 2, playerId, name: playerId.replace(/\d+$/, ""), mode: "ranked", race: "moths", ...extra });
    assert.equal(reply.status, 200, JSON.stringify(reply.body));
    return (reply.body as { ticket: string }).ticket;
  };
  const poll = (ticket: string) => route("GET", `/api/queue/${ticket}`);
  const state = (ticket: string): any => poll(ticket).body;
  const wait = (ms: number, keep: string[] = []) => {
    for (let left = ms; left > 0; left -= 1000) {
      clock.t += Math.min(1000, left);
      for (const timer of timers) if (timer.live && timer.at <= clock.t) { timer.live = false; timer.fn(); }
      for (const t of keep) poll(t);
      host.pulse();
      queue.pulse();
    }
  };
  /** Gives a player a rating this season for a race. */
  const rate = (playerId: string, race: string, rating: number) => {
    const player = db.player(playerId)?.id ?? db.addPlayer({ playerId, displayName: playerId, createdAt: SEASON_START });
    db.db
      .prepare("insert into ratings (player, season, race, rating, deviation, volatility, games, updated_at) values (?, ?, ?, ?, 100, 0.06, 10, ?)")
      .run(player, season, race, rating, SEASON_START.toISOString());
  };
  return { db, season, clock, host, queue, rolls, logs, route, enter, poll, state, wait, rate };
}

type Setup = ReturnType<typeof setup>;

/** Both players' sockets on a matched room, as the server would bind them; `ended` from a
 * game discards the room as the WebSocket handler does. */
function seats(s: Setup, a: string, b: string) {
  const ta = s.state(a), tb = s.state(b);
  assert.equal(ta.state, "matched");
  const room = s.host.get(ta.room.id)!;
  const inbox = new Map<string, ServerMsg[]>();
  const closed = new Set<string>();
  const bind = (token: string) => {
    inbox.set(token, []);
    const send = (msg: ServerMsg) => inbox.get(token)!.push(msg);
    return room.bind(token, send, { close: () => { closed.add(token); room.unbind(token, send); } });
  };
  const say = (token: string, msg: ClientMsg) => {
    room.ingest(token, msg);
    if (room.status === "ended") s.host.discard(room.id);
  };
  const got = <T extends ServerMsg["type"]>(token: string, type: T) =>
    (inbox.get(token) ?? []).filter((m): m is Extract<ServerMsg, { type: T }> => m.type === type);
  return { room, a: ta.token as string, b: tb.token as string, bind, say, got, closed };
}

/** A match from the queue under way: both bound, ready, past `go`. */
function playing(mode: "ranked" | "unranked" = "ranked") {
  const s = setup();
  const a = s.enter("Ann1", { mode }), b = s.enter("Bob2", { mode, race: "snails" });
  s.wait(1000, [a, b]);
  const r = seats(s, a, b);
  r.bind(r.a);
  r.bind(r.b);
  r.say(r.a, content);
  r.say(r.b, content);
  assert.equal(r.got(r.a, "go").length, 1);
  return { ...s, ...r };
}

const ended = (winner: number | null, tick = 24_000): ClientMsg => ({ type: "ended", outcome: { winner, defeated: winner === null ? [] : [1 - winner] }, tick, checksum: 0 });
const recorded = (db: LadderDb) => db.matches({}).matches;

test("the window: 150 + 25 per second waited, at most 1000, and anyone after 120 s", () => {
  assert.deepEqual([0, 1, 10, 34, 60, 200].map(searchRange), [150, 175, 400, 1000, 1000, 1000]);
  assert.ok(accepts(150, 0) && !accepts(151, 0));
  assert.ok(!accepts(1001, 119) && accepts(5000, 120));

  // 300 apart: they pair once the longer wait reaches 6 s.
  const s = setup();
  s.rate("Ann1", "moths", 1500);
  s.rate("Bob2", "moths", 1800);
  const a = s.enter("Ann1");
  s.wait(3000, [a]);
  const b = s.enter("Bob2");
  s.wait(2000, [a, b]);
  assert.deepEqual(s.state(a), { state: "searching", seconds: 5, range: 275 });
  assert.deepEqual(s.state(b), { state: "searching", seconds: 2, range: 200 });
  s.wait(1000, [a, b]);
  assert.equal(s.state(a).state, "matched");
  assert.equal(s.state(b).state, "matched");

  // 1500 apart: never within the window, but after 120 s any two pair.
  const far = setup();
  far.rate("Ann1", "moths", 1000);
  far.rate("Bob2", "moths", 2500);
  const x = far.enter("Ann1"), y = far.enter("Bob2");
  far.wait(119_000, [x, y]);
  assert.equal(far.state(x).state, "searching");
  far.wait(1000, [x, y]);
  assert.equal(far.state(x).state, "matched");
});

test("longest waiting first, each with the closest-rated partner it may take", () => {
  // The oldest ticket picks first: Old takes N1, though N1 and N2 are closer to each other.
  const s = setup();
  s.rate("Old1", "moths", 1500);
  s.rate("New1", "moths", 1600);
  s.rate("New2", "moths", 1605);
  const old = s.enter("Old1");
  s.wait(2000, [old]);
  const n1 = s.enter("New1"), n2 = s.enter("New2");
  s.wait(1000, [old, n1, n2]);
  const matched = s.state(old);
  assert.equal(matched.state, "matched");
  assert.equal(matched.you.player, 0);
  assert.deepEqual(matched.room.slots.map((x: any) => x.playerId), ["Old1", "New1"]);
  assert.equal(s.state(n2).state, "searching");

  // Among those it may take, the closest: A takes C (20 apart) over B (140) and D (30); B then D.
  const t = setup();
  for (const [id, r] of [["A1", 1500], ["B1", 1640], ["C1", 1520], ["D1", 1530]] as const) t.rate(id, "moths", r);
  const tickets = ["A1", "B1", "C1", "D1"].map((id) => t.enter(id));
  t.wait(1000, tickets);
  const rooms = tickets.map((x) => t.state(x).room);
  assert.deepEqual(rooms[0].slots.map((x: any) => x.playerId), ["A1", "C1"]);
  assert.deepEqual(rooms[1].slots.map((x: any) => x.playerId), ["B1", "D1"]);
});

test("a Player ID never meets itself, and entering again replaces its search", () => {
  const s = setup();
  const first = s.enter("Ann1");
  const again = s.enter("ann1", { mode: "unranked" });
  assert.deepEqual(s.poll(first), { status: 404, body: { error: "not_found" } });
  assert.deepEqual((s.route("GET", "/api/queue").body as any).searching, { ranked: 0, unranked: 1 });
  const third = s.enter("Ann1", { mode: "unranked" });
  s.wait(130_000, [again, third]);
  assert.equal(s.poll(again).status, 404);
  assert.equal(s.state(third).state, "searching");
});

test("the map: a random pool map neither vetoed, the whole pool when both vetoes empty it", () => {
  // Pool by name: Dune, Marsh, Pond. Vetoing Pond and Marsh leaves Dune, whatever the roll.
  const s = setup();
  s.rolls.push(0.99);
  const a = s.enter("Ann1", { veto: "pond" }), b = s.enter("Bob2", { veto: "marsh" });
  s.wait(1000, [a, b]);
  assert.equal(s.state(a).room.mapId, "dune");

  const t = setup();
  t.db.setPool(["pond", "marsh"]);
  t.rolls.push(0.99);
  const c = t.enter("Ann1", { veto: "pond" }), d = t.enter("Bob2", { veto: "marsh" });
  t.wait(1000, [c, d]);
  assert.equal(t.state(c).room.mapId, "pond");
  assert.equal(t.route("POST", "/api/queue", { protocol: 2, playerId: "Cid3", name: "Cid", mode: "ranked", race: "moths", veto: "dune" }).status, 400);
});

test("Random: matched by the mean over the races and resolved uniformly when matched", () => {
  const s = setup();
  s.rate("Ann1", "moths", 1600);
  // Snails never played count as the initial rating.
  assert.equal(s.queue.ratingOf("Ann1", "random"), 1550);
  assert.equal(s.queue.ratingOf("Ann1", "snails"), 1500);
  assert.equal(s.queue.ratingOf("Nobody", "random"), 1500);
  // Rolls: the map, then each Random seat in seat order.
  s.rolls.push(0, 0.99, 0.2);
  const a = s.enter("Ann1", { race: "random" }), b = s.enter("Bob2", { race: "random" });
  s.wait(1000, [a, b]);
  assert.deepEqual(s.state(a).room.slots.map((x: any) => x.race), ["snails", "moths"]);
});

test("a search nobody asks about for 15 s is dropped; a matched ticket stays readable 60 s", () => {
  const s = setup();
  const a = s.enter("Ann1");
  s.wait(10_000);
  assert.equal(s.state(a).seconds, 10);
  s.wait(14_000);
  assert.equal(s.state(a).state, "searching");
  s.wait(15_000);
  assert.equal(s.poll(a).status, 404);

  const b = s.enter("Bob2"), c = s.enter("Cid3");
  s.wait(1000, [b, c]);
  const matchedAt = s.clock.t;
  s.wait(59_000);
  assert.equal(s.state(b).state, "matched");
  assert.equal(s.clock.t - matchedAt, 59_000);
  s.wait(1000);
  assert.equal(s.poll(b).status, 404);
  assert.equal(s.poll(c).status, 404);
});

test("the matched room starts by itself once both sockets are bound", () => {
  const s = setup();
  const a = s.enter("Ann1"), b = s.enter("Bob2", { race: "snails" });
  s.wait(1000, [a, b]);
  const r = seats(s, a, b);
  const view = s.state(b).room;
  assert.equal(view.queue, "ranked");
  assert.equal(view.state, "waiting");
  assert.equal(view.spectatorPlaces, 2);
  assert.equal(view.observerDelay, 120);
  assert.deepEqual(view.slots.map((x: any) => [x.player, x.kind, x.name, x.playerId, x.race, x.team, x.color]), [
    [0, "human", "Ann", "Ann1", "moths", 1, 0],
    [1, "human", "Bob", "Bob2", "snails", 2, 1],
  ]);
  assert.deepEqual(s.state(b).you, { role: "player", player: 1, name: "Bob" });

  r.bind(r.a);
  assert.equal(r.got(r.a, "start").length, 0);
  r.bind(r.b);
  for (const t of [r.a, r.b]) assert.equal(r.got(t, "start").length, 1);
  const config = r.got(r.a, "start")[0]!.config;
  assert.equal(config.queue, "ranked");
  assert.equal(config.observerDelay, 120 * 40);
  assert.deepEqual(config.colors, [0, 1]);
  assert.deepEqual(config.slots.map((x) => [x.player, x.kind, x.race, x.team, x.hero]), [
    [0, "human", "moths", 1, undefined],
    [1, "human", "snails", 2, undefined],
  ]);
  assert.equal(r.room.status, "playing");

  // Unranked: no observer delay.
  const u = setup();
  const c = u.enter("Cid3", { mode: "unranked" }), d = u.enter("Dee4", { mode: "unranked" });
  u.wait(1000, [c, d]);
  assert.equal(u.state(c).room.queue, "unranked");
  assert.equal(u.state(c).room.observerDelay, 0);
});

test("a matched room whose second player does not connect within 30 s is cancelled", () => {
  const s = setup();
  const a = s.enter("Ann1"), b = s.enter("Bob2");
  s.wait(1000, [a, b]);
  const r = seats(s, a, b);
  r.bind(r.a);
  s.wait(29_000);
  assert.equal(r.got(r.a, "error").length, 0);
  s.wait(1000);
  assert.deepEqual(r.got(r.a, "error").map((m) => m.code), ["MATCH_CANCELLED"]);
  assert.ok(r.closed.has(r.a));
  assert.equal(r.got(r.a, "start").length, 0);
  assert.equal(s.host.get(r.room.id), undefined);
  // Nothing to record from a match that never was.
  s.wait(31_000);
  assert.equal(recorded(s.db).length, 0);
});

test("a queue's room refuses lobby messages", () => {
  const s = setup();
  const a = s.enter("Ann1"), b = s.enter("Bob2");
  s.wait(1000, [a, b]);
  const r = seats(s, a, b);
  r.bind(r.a);
  const lobby: ClientMsg[] = [
    { type: "selectRace", race: "snails" },
    { type: "selectHero", hero: "hero.moth" },
    { type: "selectTeam", team: 2 },
    { type: "selectColor", color: 5 },
    { type: "setSeat", player: 1, seat: "closed" },
    { type: "setSpectatorPlaces", count: 4 },
    { type: "setObserverDelay", seconds: 0 },
    { type: "kick", player: 1 },
    { type: "takeSeat", player: 1 },
    { type: "spectate" },
    { type: "startMatch" },
  ];
  for (const msg of lobby) r.say(r.a, msg);
  assert.deepEqual(r.got(r.a, "error").map((m) => m.code), lobby.map(() => "LOBBY_REFUSED"));
  assert.deepEqual(r.room.start(r.a), { error: "queue_room" });
  assert.deepEqual(r.room.restart(r.a), { error: "queue_room" });
  const view = r.room.view();
  assert.deepEqual(view.slots.map((x) => [x.race, x.team, x.color]), [["moths", 1, 0], ["moths", 2, 1]]);
  assert.equal(view.observerDelay, 120);
  assert.equal(view.state, "waiting");
});

test("a ranked result counts when both reports agree, also one arriving after the room ended", () => {
  const p = playing();
  p.say(p.a, ended(1));
  // The room ends for everyone at the first report, as before.
  assert.equal(p.got(p.b, "ended").length, 1);
  assert.equal(p.host.get(p.room.id), undefined);
  assert.equal(recorded(p.db).length, 0);
  p.wait(5000);
  p.say(p.b, ended(1));
  const [match] = recorded(p.db);
  assert.ok(match);
  assert.equal(match.map, p.room.mapId);
  assert.equal(match.duration, 600);
  assert.equal(match.replay, null);
  assert.deepEqual(match.sides.map((x) => [x.slot, x.playerId, x.displayName, x.race, x.hero, x.won]), [
    [0, "Ann1", "Ann", "moths", null, false],
    [1, "Bob2", "Bob", "snails", null, true],
  ]);
  const bob = p.db.player("Bob2")!;
  assert.deepEqual(p.db.ratings(bob.id, p.season).map((x) => [x.race, x.games, x.wins]), [["snails", 1, 1]]);
  // Later reports change nothing.
  p.say(p.b, ended(0));
  p.wait(31_000);
  assert.equal(recorded(p.db).length, 1);
});

test("one report counts when the other player is away", () => {
  const p = playing();
  // Leaving a ranked match: the leaver is away, and the other game reports the outcome.
  p.room.leave(p.b);
  assert.ok(p.room.isAway(1));
  p.say(p.a, ended(0));
  const [match] = recorded(p.db);
  assert.deepEqual(match?.sides.map((x) => [x.playerId, x.won]), [["Ann1", true], ["Bob2", false]]);
});

test("one report counts after 30 s without the other", () => {
  const p = playing();
  p.say(p.a, ended(0));
  p.wait(29_000);
  assert.equal(recorded(p.db).length, 0);
  p.wait(1000);
  assert.equal(recorded(p.db).length, 1);
  // A report after the window is too late to change it.
  p.say(p.b, ended(1));
  p.wait(31_000);
  assert.equal(recorded(p.db).length, 1);
  assert.equal(recorded(p.db)[0]!.sides[0]!.won, true);
});

test("reports that disagree record nothing and are logged as disputed", () => {
  const p = playing();
  p.say(p.a, ended(0));
  p.say(p.b, ended(1));
  p.wait(31_000);
  assert.equal(recorded(p.db).length, 0);
  assert.ok(p.logs.some((l) => l.includes("disputed")));
  assert.equal(p.db.player("Ann1"), null);
});

test("unranked matches and seasons' gaps record nothing", () => {
  const u = playing("unranked");
  u.say(u.a, ended(0));
  u.say(u.b, ended(0));
  u.wait(31_000);
  assert.equal(recorded(u.db).length, 0);

  const s = setup({ withSeason: false });
  const a = s.enter("Ann1"), b = s.enter("Bob2");
  s.wait(1000, [a, b]);
  const r = seats(s, a, b);
  r.bind(r.a);
  r.bind(r.b);
  r.say(r.a, content);
  r.say(r.b, content);
  r.say(r.a, ended(0));
  r.say(r.b, ended(0));
  assert.equal(recorded(s.db).length, 0);
});

test("the queue routes answer 503 without a ladder", () => {
  for (const [method, path] of [["GET", "/api/queue"], ["POST", "/api/queue"], ["GET", "/api/queue/t0"], ["DELETE", "/api/queue/t0"]])
    assert.deepEqual(queueRoute(null, method!, path!, {}), { status: 503, body: { error: "ladder_unavailable" } });
  assert.equal(queueRoute(null, "GET", "/api/rooms"), null);
  assert.equal(queueRoute(null, "PUT", "/api/queue"), null);
});

test("the queue routes: shapes and errors", () => {
  const s = setup();
  assert.deepEqual(s.route("GET", "/api/queue"), {
    status: 200,
    body: {
      pool: [{ id: "dune", name: "Dune" }, { id: "marsh", name: "Marsh" }, { id: "pond", name: "Pond" }],
      races: [{ id: "moths", name: "Moths" }, { id: "snails", name: "Snails" }],
      searching: { ranked: 0, unranked: 0 },
    },
  });
  const body = { protocol: 2, playerId: "Ann1", name: "Ann", mode: "ranked", race: "moths" };
  const post = (b: unknown) => s.route("POST", "/api/queue", b);
  assert.deepEqual(post({ ...body, protocol: 1 }), { status: 426, body: { error: "update_required", message: "Update Under the Canopy to play online." } });
  assert.deepEqual(post({ ...body, playerId: "Ann 1" }), { status: 400, body: { error: "bad_player_id" } });
  assert.deepEqual(post({ ...body, mode: "casual" }), { status: 400, body: { error: "bad_mode" } });
  assert.deepEqual(post({ ...body, race: "wasps" }), { status: 400, body: { error: "bad_race" } });
  assert.deepEqual(post({ ...body, veto: "cave" }), { status: 400, body: { error: "bad_veto" } });
  assert.deepEqual(post(new Error("bad json")), { status: 400, body: { error: "bad_json" } });

  assert.deepEqual(post({ ...body, veto: "pond" }), { status: 200, body: { ticket: "t0" } });
  assert.deepEqual(post({ ...body, playerId: "Bob2", name: "Bob", mode: "unranked", race: "random" }), { status: 200, body: { ticket: "t1" } });
  assert.deepEqual((s.route("GET", "/api/queue").body as any).searching, { ranked: 1, unranked: 1 });
  assert.deepEqual(s.poll("t0"), { status: 200, body: { state: "searching", seconds: 0, range: 150 } });
  assert.deepEqual(s.poll("nope"), { status: 404, body: { error: "not_found" } });
  assert.deepEqual(s.route("DELETE", "/api/queue/t1"), { status: 204 });
  assert.equal(s.poll("t1").status, 404);
  assert.deepEqual(s.route("DELETE", "/api/queue/t1"), { status: 204 });

  // Matched: the shape of joining a room. Deleting it does not undo the match.
  const b = s.enter("Cid3");
  s.wait(1000, ["t0", b]);
  const matched = s.poll("t0");
  assert.equal(matched.status, 200);
  const m = matched.body as any;
  assert.deepEqual(Object.keys(m).sort(), ["room", "state", "token", "you"]);
  assert.equal(m.state, "matched");
  assert.deepEqual(m.you, { role: "player", player: 0, name: "Ann" });
  assert.equal(typeof m.token, "string");
  assert.equal(m.room.queue, "ranked");
  assert.equal(m.room.name, "Ranked 1v1");
  assert.deepEqual(s.route("DELETE", "/api/queue/t0"), { status: 204 });
  assert.equal(s.state("t0").state, "matched");

  // An empty pool takes nobody.
  s.db.setPool([]);
  assert.deepEqual(post(body), { status: 503, body: { error: "no_pool" } });
  assert.deepEqual((s.route("GET", "/api/queue").body as any).pool, []);
});

test("the pool flag: added to an older database, kept when a map is written again", () => {
  const file = join(mkdtempSync(join(tmpdir(), "ladder-pool-")), "ladder.sqlite");
  const old = new DatabaseSync(file);
  old.exec("create table maps (id text primary key, name text not null, preview text); insert into maps values ('pond', 'Pond', null)");
  old.close();
  const db = new LadderDb(file);
  assert.deepEqual(db.pool(), []);
  db.setPool(["pond"]);
  db.putMap({ id: "pond", name: "Pond II", preview: null });
  assert.deepEqual(db.pool(), [{ id: "pond", name: "Pond II", preview: null }]);
  assert.throws(() => db.setPool(["pond", "nowhere"]));
  assert.deepEqual(db.pool().map((m) => m.id), ["pond"]);
  db.close();
});
