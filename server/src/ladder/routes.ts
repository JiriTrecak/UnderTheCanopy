/**
 * The ladder website and its JSON API under `/ladder` (docs/server.md#ladder): pages,
 * `/ladder/api/…`, replay downloads at `/ladder/replays/<match>`, the site's own files at
 * `/ladder/static/…` and copied content media at `/ladder/media/…`.
 */
import { createReadStream, existsSync, mkdirSync, statSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { dirname, extname, join, normalize, sep } from "node:path";
import { LadderDb, type Season } from "./db";
import { LEAGUES, PLACEMENT_MATCHES, REPLAY_DAYS, SHOW_RATING, TIERS, replayKept, standing, standingName } from "./leagues";
import { matchesPage, notFoundPage, overviewPage, playerPage, racePage, searchPage, ROOT, type Context } from "./pages";
import { ladderPaths, type LadderPaths } from "./paths";

const TYPES: Record<string, string> = {
  ".css": "text/css; charset=utf-8",
  ".webp": "image/webp",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
};
const BOARD_PAGE = 50;
const MATCH_PAGE = 30;
const PROFILE_PAGE = 20;
const TOP = 10;

export type Ladder = {
  /** The database the site reads; the queues rate and record through it too. */
  db: LadderDb;
  handle(req: IncomingMessage, res: ServerResponse, url: URL): Promise<void>;
  close(): void;
};

export function createLadder(opts: { paths?: LadderPaths; now?: () => Date } = {}): Ladder {
  const paths = opts.paths ?? ladderPaths();
  const now = opts.now ?? (() => new Date());
  mkdirSync(dirname(paths.db), { recursive: true });
  const db = new LadderDb(paths.db);

  const context = (): Context => ({
    races: db.races(),
    heroes: db.heroes(),
    maps: db.maps(),
    now: now(),
    hasRaceImage: (race) => /^[a-z0-9-]+$/.test(race) && existsSync(join(paths.site, "img", `race-${race}.webp`)),
  });

  const pageOf = (url: URL) => Math.max(1, Math.floor(Number(url.searchParams.get("page") ?? 1)) || 1);
  const leagueParam = (url: URL) => {
    const v = url.searchParams.get("league");
    if (v === null || v === "") return null;
    const i = Number(v);
    return Number.isInteger(i) && i >= 0 && i < LEAGUES.length ? i : null;
  };

  function overview(ctx: Context, season: Season | null) {
    if (!season) return null;
    const top: Record<string, ReturnType<typeof db.leaderboard>["rows"]> = {};
    for (const r of ctx.races) top[r.id] = db.leaderboard(season.id, r.id, { limit: TOP }).rows;
    return {
      season,
      stats: db.stats(season.id, ctx.now),
      top,
      distribution: db.distribution(season.id),
      raceRecords: db.raceRecords(season.id),
      heroRecords: db.heroRecords(season.id),
      recent: db.matches({ season: season.id, limit: 8 }).matches,
    };
  }

  function raceBoard(ctx: Context, season: Season | null, raceId: string, url: URL) {
    const race = ctx.races.find((r) => r.id === raceId);
    if (!race) return null;
    const league = leagueParam(url);
    const page = pageOf(url);
    const distribution = season ? db.distribution(season.id)[race.id] ?? [] : [];
    const board = season ? db.leaderboard(season.id, race.id, { league, offset: (page - 1) * BOARD_PAGE, limit: BOARD_PAGE }) : { total: 0, rows: [] };
    return {
      race,
      season,
      board,
      page,
      pages: Math.max(1, Math.ceil(board.total / BOARD_PAGE)),
      league,
      placed: distribution.reduce((s, v) => s + (v ?? 0), 0),
      distribution: LEAGUES.map((_, i) => distribution[i] ?? 0),
      record: season ? db.raceRecords(season.id)[race.id] : undefined,
    };
  }

  function profile(ctx: Context, season: Season | null, playerId: string, url: URL) {
    const player = db.player(playerId);
    if (!player) return null;
    const page = pageOf(url);
    const ratings = season ? db.ratings(player.id, season.id) : [];
    const totals: Record<string, number> = {};
    if (season) for (const r of ctx.races) totals[r.id] = db.leaderboard(season.id, r.id, { limit: 0 }).total;
    const list = db.matches({ player: player.id, offset: (page - 1) * PROFILE_PAGE, limit: PROFILE_PAGE });
    return { player, season, ratings, totals, matches: list.matches, page, pages: Math.max(1, Math.ceil(list.total / PROFILE_PAGE)) };
  }

  async function handle(req: IncomingMessage, res: ServerResponse, url: URL): Promise<void> {
    if (req.method !== "GET" && req.method !== "HEAD") {
      sendJson(res, 405, { error: "method_not_allowed" });
      return;
    }
    const path = url.pathname.length > ROOT.length && url.pathname.endsWith("/") ? url.pathname.slice(0, -1) : url.pathname;
    if (path !== url.pathname) {
      redirect(res, path + url.search);
      return;
    }
    const rest = path.slice(ROOT.length);

    // Files first: they need no database.
    const file = /^\/static\/(.+)$/.exec(rest);
    if (file) return serveFile(req, res, paths.site, decode(file[1]!), "public, max-age=3600");
    const media = /^\/media\/(.+)$/.exec(rest);
    if (media) return serveFile(req, res, paths.media, decode(media[1]!), "public, max-age=86400");

    const ctx = context();
    const season = db.season(ctx.now);

    // JSON API.
    if (rest === "/api/season") {
      const o = overview(ctx, season);
      sendJson(res, 200, {
        season,
        leagues: LEAGUES.map((l, i) => ({ index: i, name: l.name, from: Number.isFinite(l.from) ? l.from : null })),
        tiers: TIERS,
        placementMatches: PLACEMENT_MATCHES,
        replayDays: REPLAY_DAYS,
        races: ctx.races.map((r) => ({ id: r.id, name: r.name })),
        stats: o?.stats ?? null,
        distribution: o?.distribution ?? {},
        top: o ? Object.fromEntries(Object.entries(o.top).map(([race, rows]) => [race, rows.map(boardJson)])) : {},
      });
      return;
    }
    const apiBoard = /^\/api\/leaderboard\/([^/]+)$/.exec(rest);
    if (apiBoard) {
      const b = raceBoard(ctx, season, decode(apiBoard[1]!), url);
      if (!b) return sendJson(res, 404, { error: "not_found" });
      sendJson(res, 200, { race: b.race.id, season: season?.number ?? null, page: b.page, pages: b.pages, total: b.board.total, rows: b.board.rows.map(boardJson) });
      return;
    }
    const apiPlayer = /^\/api\/players\/([^/]+)$/.exec(rest);
    if (apiPlayer) {
      const p = profile(ctx, season, decode(apiPlayer[1]!), url);
      if (!p) return sendJson(res, 404, { error: "not_found" });
      sendJson(res, 200, {
        playerId: p.player.playerId,
        displayName: p.player.displayName,
        season: season?.number ?? null,
        ratings: p.ratings.map((r) => ({
          race: r.race,
          ...(SHOW_RATING ? { rating: Math.round(r.rating), deviation: Math.round(r.deviation) } : {}),
          games: r.games,
          wins: r.wins,
          losses: r.losses,
          standing: standingName(standing(r.rating, r.games)),
          league: r.league,
          tier: r.tier,
          rank: r.rank,
        })),
        matches: p.matches.map((m) => matchJson(m, ctx.now)),
        page: p.page,
        pages: p.pages,
      });
      return;
    }
    if (rest === "/api/players") {
      const q = (url.searchParams.get("q") ?? "").trim().slice(0, 32);
      sendJson(res, 200, { players: q ? db.search(q).map((p) => ({ playerId: p.playerId, displayName: p.displayName })) : [] });
      return;
    }
    if (rest === "/api/matches") {
      const page = pageOf(url);
      const playerId = url.searchParams.get("player");
      const player = playerId ? db.player(playerId) : null;
      if (playerId && !player) return sendJson(res, 404, { error: "not_found" });
      const race = url.searchParams.get("race") ?? undefined;
      const list = db.matches({ season: player ? undefined : season?.id, player: player?.id, race, offset: (page - 1) * MATCH_PAGE, limit: MATCH_PAGE });
      sendJson(res, 200, { page, pages: Math.max(1, Math.ceil(list.total / MATCH_PAGE)), total: list.total, matches: list.matches.map((m) => matchJson(m, ctx.now)) });
      return;
    }
    if (rest.startsWith("/api/")) return sendJson(res, 404, { error: "not_found" });

    // Replays.
    const replay = /^\/replays\/(\d+)$/.exec(rest);
    if (replay) return serveReplay(req, res, ctx, Number(replay[1]));

    // Pages.
    if (rest === "") return sendHtml(res, 200, overviewPage(ctx, overview(ctx, season)));
    const race = /^\/race\/([^/]+)$/.exec(rest);
    if (race) {
      const b = raceBoard(ctx, season, decode(race[1]!), url);
      return b ? sendHtml(res, 200, racePage(ctx, b)) : sendHtml(res, 404, notFoundPage(ctx, "This race"));
    }
    const player = /^\/player\/([^/]+)$/.exec(rest);
    if (player) {
      const p = profile(ctx, season, decode(player[1]!), url);
      return p ? sendHtml(res, 200, playerPage(ctx, p)) : sendHtml(res, 404, notFoundPage(ctx, "This player"));
    }
    if (rest === "/matches") {
      const page = pageOf(url);
      const raceId = url.searchParams.get("race");
      const known = raceId && ctx.races.some((r) => r.id === raceId) ? raceId : null;
      const list = season ? db.matches({ season: season.id, race: known ?? undefined, offset: (page - 1) * MATCH_PAGE, limit: MATCH_PAGE }) : { total: 0, matches: [] };
      return sendHtml(res, 200, matchesPage(ctx, { ...list, page, pages: Math.max(1, Math.ceil(list.total / MATCH_PAGE)), race: known, season }));
    }
    if (rest === "/search") {
      const q = (url.searchParams.get("q") ?? "").trim().slice(0, 32);
      const found = q ? db.search(q) : [];
      const exact = found.find((p) => p.playerId.toLowerCase() === q.toLowerCase());
      if (exact) return redirect(res, `${ROOT}/player/${encodeURIComponent(exact.playerId)}`);
      return sendHtml(res, 200, searchPage(ctx, q, found));
    }
    sendHtml(res, 404, notFoundPage(ctx));
  }

  function serveReplay(req: IncomingMessage, res: ServerResponse, ctx: Context, id: number): void {
    const m = db.match(id);
    if (!m || !m.replay) return sendJson(res, 404, { error: "not_found" });
    if (!replayKept(new Date(m.playedAt), ctx.now)) return sendJson(res, 410, { error: "expired", message: `Replays are kept ${REPLAY_DAYS} days.` });
    const names = m.sides.map((s) => s.playerId).join(" vs ");
    const date = m.playedAt.slice(0, 10);
    if (m.replay === "sample") {
      return streamFile(req, res, join(paths.site, "sample-replay.txt"), {
        "content-type": "text/plain; charset=utf-8",
        "content-disposition": attachment(`SAMPLE ${date} ${names} (not a replay).txt`),
      });
    }
    if (!/^[A-Za-z0-9._-]+\.utcr$/.test(m.replay)) return sendJson(res, 404, { error: "not_found" });
    const mapName = ctx.maps.get(m.map)?.name ?? m.map;
    streamFile(req, res, join(paths.replays, m.replay), {
      "content-type": "application/vnd.canopy.replay",
      "content-disposition": attachment(`${mapName} ${date} ${names}.utcr`),
    });
  }

  return { db, handle, close: () => db.close() };
}

function boardJson(r: { rank: number; playerId: string; displayName: string; rating: number; deviation: number; games: number; wins: number; losses: number; league: number; tier: number; hero: string | null }) {
  return {
    rank: r.rank,
    playerId: r.playerId,
    displayName: r.displayName,
    ...(SHOW_RATING ? { rating: Math.round(r.rating) } : {}),
    games: r.games,
    wins: r.wins,
    losses: r.losses,
    league: r.league,
    tier: r.tier,
    standing: standingName({ placed: true, league: r.league, tier: r.tier }),
    hero: r.hero,
  };
}

function matchJson(m: import("./db").Match, now: Date) {
  const kept = !!m.replay && replayKept(new Date(m.playedAt), now);
  return {
    id: m.id,
    playedAt: m.playedAt,
    map: m.map,
    duration: m.duration,
    replay: kept ? `${ROOT}/replays/${m.id}` : null,
    sampleReplay: m.replay === "sample",
    players: m.sides.map((s) => ({
      playerId: s.playerId,
      displayName: s.displayName,
      race: s.race,
      hero: s.hero,
      won: s.won,
      ...(SHOW_RATING ? { ratingChange: Math.round(s.ratingAfter - s.ratingBefore) } : {}),
    })),
  };
}

function decode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return "";
  }
}

/** An attachment header with a plain fallback and the UTF-8 name. */
function attachment(name: string): string {
  const plain = name.replace(/[^A-Za-z0-9 ._()-]/g, "-");
  return `attachment; filename="${plain}"; filename*=UTF-8''${encodeURIComponent(name)}`;
}

function serveFile(req: IncomingMessage, res: ServerResponse, root: string, relative: string, cache: string): void {
  const full = normalize(join(root, relative));
  const type = TYPES[extname(full).toLowerCase()];
  if (!relative || relative.includes("\0") || !full.startsWith(normalize(root) + sep) || !type) return sendJson(res, 404, { error: "not_found" });
  streamFile(req, res, full, { "content-type": type, "cache-control": cache });
}

function streamFile(req: IncomingMessage, res: ServerResponse, file: string, headers: Record<string, string>): void {
  let size: number;
  try {
    const st = statSync(file);
    if (!st.isFile()) throw Error("not a file");
    size = st.size;
  } catch {
    sendJson(res, 404, { error: "not_found" });
    return;
  }
  res.writeHead(200, { ...headers, "content-length": String(size), "x-content-type-options": "nosniff" });
  if (req.method === "HEAD") {
    res.end();
    return;
  }
  createReadStream(file).pipe(res);
}

function sendHtml(res: ServerResponse, status: number, body: string): void {
  res.writeHead(status, {
    "content-type": "text/html; charset=utf-8",
    "cache-control": "no-cache",
    "x-content-type-options": "nosniff",
    "content-security-policy": "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'none'; base-uri 'none'; form-action 'self'",
  });
  res.end(body);
}

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { "content-type": "application/json", "access-control-allow-origin": "*", "cache-control": "no-cache" });
  res.end(JSON.stringify(body));
}

function redirect(res: ServerResponse, location: string): void {
  res.writeHead(302, { location });
  res.end();
}
