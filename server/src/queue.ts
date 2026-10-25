/**
 * The ranked and unranked 1v1 queues (D107, docs/server.md#queues-and-matchmaking): tickets,
 * pairing once a second by a rating window that widens with the wait, the map by veto,
 * Random races resolved, the room both players are put in, and a ranked match's result
 * recorded in the ladder once the games' reports agree. Ratings and the map pool come from
 * the ladder's database; the clock and the random source are injected.
 */
import type { HostedMatch, MatchHost, EndedReport } from "./host";
import type { LadderDb, MapRow, Race, Season } from "./ladder/db";
import { INITIAL } from "./ladder/glicko";
import { QUEUE_MODES, TICK_MS, admit, type ClientIdentity, type QueueMode } from "./protocol";

/** The race a ticket may name to be given one at random. */
export const RANDOM = "random";
/** The rating difference two tickets may have: `150 + 25 × s`, `s` the longer wait in
 * seconds, at most 1000; after 120 s any two pair. */
export const RANGE_FROM = 150;
export const RANGE_PER_SECOND = 25;
export const RANGE_MAX = 1000;
export const ANY_PARTNER_AFTER_S = 120;
/** A searching ticket nobody asked about for this long is dropped (ms). */
export const SEARCH_STALE_MS = 15_000;
/** A matched ticket stays readable this long (ms). */
export const MATCHED_KEPT_MS = 60_000;
/** How long a ranked match waits for the second report, and for any after the room ended (ms). */
export const REPORT_WINDOW_MS = 30_000;

/** The rating difference a search accepts after `seconds` of waiting (shown to the client). */
export function searchRange(seconds: number): number {
  return Math.min(RANGE_MAX, RANGE_FROM + RANGE_PER_SECOND * seconds);
}

/** Whether two tickets `gap` apart pair once the longer of them waited `seconds`. */
export function accepts(gap: number, seconds: number): boolean {
  return seconds >= ANY_PARTNER_AFTER_S || gap <= searchRange(seconds);
}

export type QueueOptions = {
  db: LadderDb;
  host: MatchHost;
  /** Milliseconds, for waits and windows (the host's clock). */
  now?: () => number;
  /** The calendar, for the season and the recorded match's time. */
  date?: () => Date;
  /** In [0, 1): the map and Random races. */
  random?: () => number;
  ticketId?: () => string;
  log?: (line: string) => void;
};

type Matched = { at: number; token: string; you: ClientIdentity; room: HostedMatch };

type Ticket = {
  id: string;
  mode: QueueMode;
  playerId: string;
  name: string;
  /** A race id or `random`. */
  race: string;
  veto: string | null;
  rating: number;
  /** When the search began, and when the client last asked about it. */
  since: number;
  polled: number;
  /** Entry order, for tickets that began at the same moment. */
  order: number;
  matched: Matched | null;
};

type Report = { winner: number | null; tick: number };

/** A ranked room's result while its reports come in. */
type Pending = {
  room: HostedMatch;
  mapId: string;
  seats: readonly [{ playerId: string; name: string; race: string }, { playerId: string; name: string; race: string }];
  reports: Map<number, Report>;
  /** When the first report came. */
  firstAt: number | null;
  /** When the room was first seen over without any report. */
  overAt: number | null;
};

/** An HTTP answer: status and JSON body (none for 204). */
export type Reply = { status: number; body?: unknown };

const sameId = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
const DISPLAY_NAME = 32;

export class Queue {
  private readonly db: LadderDb;
  private readonly host: MatchHost;
  private readonly now: () => number;
  private readonly date: () => Date;
  private readonly random: () => number;
  private readonly ticketId: () => string;
  private readonly logLine: (line: string) => void;
  private readonly tickets = new Map<string, Ticket>();
  private readonly pending = new Map<string, Pending>();
  private order = 0;

  constructor(options: QueueOptions) {
    this.db = options.db;
    this.host = options.host;
    this.now = options.now ?? (() => performance.now());
    this.date = options.date ?? (() => new Date());
    this.random = options.random ?? Math.random;
    this.ticketId = options.ticketId ?? (() => crypto.randomUUID());
    this.logLine = options.log ?? ((line) => console.log(line));
  }

  private log(line: string) {
    this.logLine(`queue: ${line}`);
  }

  /** The season under way, or none between seasons. */
  private season(at: Date): Season | null {
    const season = this.db.season(at);
    return season && at < new Date(season.endsAt) ? season : null;
  }

  // ---- Routes ----

  /** `GET /api/queue`. */
  info(): Reply {
    const searching = { ranked: 0, unranked: 0 };
    for (const t of this.tickets.values()) if (!t.matched) searching[t.mode]++;
    return {
      status: 200,
      body: {
        pool: this.db.pool().map((m) => ({ id: m.id, name: m.name })),
        races: this.db.races().map((r) => ({ id: r.id, name: r.name })),
        searching,
      },
    };
  }

  /** `POST /api/queue`: a ticket, replacing the player's searching one. */
  enter(body: unknown): Reply {
    const refused = admit(body);
    if (refused) return refused;
    const o = body as Record<string, unknown>;
    const playerId = o.playerId as string;
    const mode = o.mode as QueueMode;
    if (!QUEUE_MODES.includes(mode)) return { status: 400, body: { error: "bad_mode" } };
    const races = this.db.races();
    const race = o.race;
    if (typeof race !== "string" || (race === RANDOM ? races.length === 0 : !races.some((r) => r.id === race)))
      return { status: 400, body: { error: "bad_race" } };
    const pool = this.db.pool();
    if (pool.length === 0) return { status: 503, body: { error: "no_pool" } };
    const veto = o.veto ?? null;
    if (veto !== null && !pool.some((m) => m.id === veto)) return { status: 400, body: { error: "bad_veto" } };
    const name = (typeof o.name === "string" ? o.name.replace(/[\u0000-\u001f\u007f]/g, " ").trim().slice(0, DISPLAY_NAME) : "") || playerId;

    for (const [id, t] of this.tickets)
      if (!t.matched && sameId(t.playerId, playerId)) {
        this.tickets.delete(id);
        this.log(`${playerId} replaced ticket ${id}`);
      }
    const now = this.now();
    const ticket: Ticket = {
      id: this.ticketId(),
      mode,
      playerId,
      name,
      race,
      veto: veto as string | null,
      rating: this.rating(playerId, race, races),
      since: now,
      polled: now,
      order: this.order++,
      matched: null,
    };
    this.tickets.set(ticket.id, ticket);
    this.log(`${name} (${playerId}) searches ${mode} as ${race}${ticket.veto ? `, vetoing ${ticket.veto}` : ""}, rating ${Math.round(ticket.rating)}`);
    return { status: 200, body: { ticket: ticket.id } };
  }

  /** `GET /api/queue/:ticket`: still searching, or the seat in the matched room. */
  poll(id: string): Reply {
    const t = this.tickets.get(id);
    if (!t) return { status: 404, body: { error: "not_found" } };
    if (t.matched) {
      const { token, you, room } = t.matched;
      return { status: 200, body: { state: "matched", token, room: room.view(), you } };
    }
    const now = this.now();
    t.polled = now;
    const seconds = this.waited(t, now);
    return { status: 200, body: { state: "searching", seconds, range: searchRange(seconds) } };
  }

  /** `DELETE /api/queue/:ticket`: stops searching; a matched ticket stays. */
  leave(id: string): Reply {
    const t = this.tickets.get(id);
    if (t && !t.matched) {
      this.tickets.delete(id);
      this.log(`${t.playerId} stopped searching ${t.mode}`);
    }
    return { status: 204 };
  }

  // ---- Matching ----

  /** Once a second: stale and old tickets go, each queue pairs, ranked results settle. */
  pulse(): void {
    const now = this.now();
    for (const [id, t] of this.tickets) {
      if (t.matched ? now - t.matched.at >= MATCHED_KEPT_MS : now - t.polled >= SEARCH_STALE_MS) {
        this.tickets.delete(id);
        if (!t.matched) this.log(`${t.playerId}'s ${t.mode} search went stale`);
      }
    }
    for (const mode of QUEUE_MODES) this.pair(mode, now);
    this.settleDue(now);
  }

  /** Whole seconds a ticket has searched. */
  private waited(t: Ticket, now: number): number {
    return Math.max(0, Math.floor((now - t.since) / 1000));
  }

  /** The rating a ticket of this player and race is matched by. */
  ratingOf(playerId: string, race: string): number {
    return this.rating(playerId, race, this.db.races());
  }

  /** A player's rating this season for `race`; for Random the mean over the races. */
  private rating(playerId: string, race: string, races: readonly Race[]): number {
    const season = this.season(this.date());
    const player = season ? this.db.player(playerId) : null;
    const rows = season && player ? this.db.ratings(player.id, season.id) : [];
    const of = (id: string) => rows.find((r) => r.race === id)?.rating ?? INITIAL.rating;
    if (race !== RANDOM) return of(race);
    return races.reduce((sum, r) => sum + of(r.id), 0) / races.length;
  }

  /** Longest waiting first, each with the closest-rated partner it may take. */
  private pair(mode: QueueMode, now: number): void {
    const searching = [...this.tickets.values()]
      .filter((t) => t.mode === mode && !t.matched)
      .sort((a, b) => a.since - b.since || a.order - b.order);
    if (searching.length < 2) return;
    const pool = this.db.pool();
    const races = this.db.races();
    if (pool.length === 0 || races.length === 0) return;
    const taken = new Set<Ticket>();
    for (const a of searching) {
      if (taken.has(a)) continue;
      let best: Ticket | null = null;
      let bestGap = Infinity;
      for (const b of searching) {
        if (b === a || taken.has(b) || sameId(a.playerId, b.playerId)) continue;
        const gap = Math.abs(a.rating - b.rating);
        if (!accepts(gap, Math.max(this.waited(a, now), this.waited(b, now)))) continue;
        if (gap < bestGap) {
          best = b;
          bestGap = gap;
        }
      }
      if (!best) continue;
      taken.add(a);
      taken.add(best);
      this.match(a, best, pool, races, now);
    }
  }

  private pick<T>(list: readonly T[]): T {
    return list[Math.min(list.length - 1, Math.floor(this.random() * list.length))]!;
  }

  /** Puts two tickets in a room: a map neither vetoed, Random races resolved. */
  private match(a: Ticket, b: Ticket, pool: readonly MapRow[], races: readonly Race[], now: number): void {
    const open = pool.filter((m) => m.id !== a.veto && m.id !== b.veto);
    const map = this.pick(open.length ? open : pool);
    const race = (t: Ticket) => (t.race === RANDOM ? this.pick(races).id : t.race);
    const seats = [
      { name: a.name, playerId: a.playerId, race: race(a) },
      { name: b.name, playerId: b.playerId, race: race(b) },
    ] as const;
    const result: Pending | null =
      a.mode === "ranked" ? { room: null!, mapId: map.id, seats, reports: new Map(), firstAt: null, overAt: null } : null;
    const { room, seats: joined } = this.host.createQueued({
      mode: a.mode,
      mapId: map.id,
      seats,
      ...(result ? { onReport: (report: EndedReport) => this.report(result, report) } : {}),
    });
    if (result) {
      result.room = room;
      this.pending.set(room.id, result);
    }
    a.matched = { at: now, room, ...joined[0] };
    b.matched = { at: now, room, ...joined[1] };
    const side = (t: Ticket, s: (typeof seats)[number]) => `${t.playerId} ${s.race} (${Math.round(t.rating)}, ${this.waited(t, now)} s)`;
    this.log(`${a.mode} match in room ${room.id} on ${map.id}: ${side(a, seats[0])} vs ${side(b, seats[1])}`);
  }

  // ---- Ranked results ----

  /** A seated player's `ended`: settles at once when both agree or the other is away. */
  private report(result: Pending, report: EndedReport): void {
    if (this.pending.get(result.room.id) !== result || result.reports.has(report.player)) return;
    const outcome = (report.outcome && typeof report.outcome === "object" ? report.outcome : {}) as Record<string, unknown>;
    const winner = outcome.winner === 0 || outcome.winner === 1 ? outcome.winner : null;
    const tick = Number.isSafeInteger(report.tick) && (report.tick as number) >= 0 ? (report.tick as number) : 0;
    result.reports.set(report.player, { winner, tick });
    this.log(`room ${result.room.id}: seat ${report.player} reports winner ${winner ?? "none"} at tick ${tick}`);
    if (result.reports.size >= 2 || result.room.isAway(1 - report.player)) this.settle(result);
    else result.firstAt ??= this.now();
  }

  /** Results whose window closed: one report recorded alone; rooms over without any forgotten. */
  private settleDue(now: number): void {
    for (const [id, result] of this.pending) {
      if (result.firstAt !== null) {
        if (now - result.firstAt >= REPORT_WINDOW_MS) this.settle(result);
        continue;
      }
      const state = result.room.status;
      if (state === "waiting" || state === "playing") continue;
      result.overAt ??= now;
      if (now - result.overAt >= REPORT_WINDOW_MS) {
        this.pending.delete(id);
        this.log(`room ${id}: no result reported, nothing recorded`);
      }
    }
  }

  private settle(result: Pending): void {
    this.pending.delete(result.room.id);
    const reports = [...result.reports.entries()];
    const room = result.room.id;
    if (reports.length === 2 && reports[0]![1].winner !== reports[1]![1].winner) {
      const said = reports.map(([seat, r]) => `seat ${seat} ${result.seats[seat]!.playerId} says ${r.winner ?? "none"}`).join(", ");
      this.log(`room ${room}: disputed result (${said}), nothing recorded; replay it with canopy replay --verify`);
      return;
    }
    const winner = reports[0]![1].winner;
    if (winner !== 0 && winner !== 1) {
      this.log(`room ${room}: no winner reported, nothing recorded`);
      return;
    }
    const tick = Math.max(...reports.map(([, r]) => r.tick));
    const at = this.date();
    const season = this.season(at);
    if (!season) {
      this.log(`room ${room}: no season under way, nothing recorded`);
      return;
    }
    try {
      const id = this.db.transaction(() => {
        const [p0, p1] = result.seats.map(
          (s) => this.db.player(s.playerId)?.id ?? this.db.addPlayer({ playerId: s.playerId, displayName: s.name, createdAt: at }),
        );
        return this.db.recordMatch({
          season: season.id,
          playedAt: at,
          map: result.mapId,
          duration: Math.round((tick * TICK_MS) / 1000),
          replay: null,
          winner,
          sides: [
            { player: p0!, race: result.seats[0].race, hero: null },
            { player: p1!, race: result.seats[1].race, hero: null },
          ],
        });
      });
      const how = reports.length === 2 ? "both reports agree" : "one report";
      this.log(`room ${room}: recorded ladder match ${id}, ${result.seats[winner].playerId} won (${how})`);
    } catch (e) {
      this.log(`room ${room}: recording failed: ${e instanceof Error ? e.message : e}`);
    }
  }
}

/** The queue's routes: null for another path or method. Without a ladder (`queue` null)
 * every queue route answers 503. `body` is the POST body, or an Error when it was not JSON. */
export function queueRoute(queue: Queue | null, method: string, path: string, body?: unknown): Reply | null {
  const one = /^\/api\/queue\/([^/]+)$/.exec(path);
  const known = path === "/api/queue" ? method === "GET" || method === "POST" : !!one && (method === "GET" || method === "DELETE");
  if (!known) return null;
  if (!queue) return { status: 503, body: { error: "ladder_unavailable" } };
  if (!one) {
    if (method === "GET") return queue.info();
    if (body instanceof Error) return { status: 400, body: { error: "bad_json" } };
    return queue.enter(body);
  }
  let id: string;
  try {
    id = decodeURIComponent(one[1]!);
  } catch {
    return method === "GET" ? { status: 404, body: { error: "not_found" } } : { status: 204 };
  }
  return method === "GET" ? queue.poll(id) : queue.leave(id);
}
