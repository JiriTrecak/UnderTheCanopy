/**
 * The ladder's SQLite database (D107): players, their ratings per race and season, and
 * ranked matches with their replays. The server knows no content (D53): races, heroes and
 * maps are rows too, written from the game's content by whatever records matches (today
 * the seed script).
 */
import { DatabaseSync } from "node:sqlite";
import { INITIAL, match as rate } from "./glicko";
import { PLACEMENT_MATCHES, standing } from "./leagues";

const SCHEMA = `
create table if not exists seasons (
  id integer primary key,
  number integer not null unique,
  name text not null,
  starts_at text not null,
  ends_at text not null
);
create table if not exists races (
  id text primary key,
  name text not null,
  description text not null default '',
  position integer not null,
  accent text not null
);
create table if not exists heroes (
  id text primary key,
  race text not null references races(id),
  name text not null,
  portrait text
);
create table if not exists maps (
  id text primary key,
  name text not null,
  preview text
);
create table if not exists players (
  id integer primary key,
  player_id text not null unique collate nocase,
  display_name text not null,
  created_at text not null
);
-- One Glicko-2 rating per player, season and race. league is an index into LEAGUES
-- (leagues.ts), null while the race is in placement.
create table if not exists ratings (
  player integer not null references players(id),
  season integer not null references seasons(id),
  race text not null references races(id),
  rating real not null,
  deviation real not null,
  volatility real not null,
  games integer not null default 0,
  wins integer not null default 0,
  losses integer not null default 0,
  league integer,
  tier integer,
  updated_at text not null,
  primary key (player, season, race)
);
create index if not exists ratings_board on ratings (season, race, rating desc);
-- replay: a file name in the replays folder, 'sample' for seeded matches, null when none.
create table if not exists matches (
  id integer primary key,
  season integer not null references seasons(id),
  played_at text not null,
  map text not null references maps(id),
  duration integer not null,
  winner integer not null,
  replay text
);
create index if not exists matches_played on matches (played_at desc);
create table if not exists match_players (
  match integer not null references matches(id) on delete cascade,
  slot integer not null,
  player integer not null references players(id),
  race text not null references races(id),
  hero text references heroes(id),
  rating_before real not null,
  rating_after real not null,
  primary key (match, slot)
);
create index if not exists match_players_player on match_players (player, match desc);
`;

export type Season = { id: number; number: number; name: string; startsAt: string; endsAt: string };
export type Race = { id: string; name: string; description: string; accent: string };
export type Hero = { id: string; race: string; name: string; portrait: string | null };
export type MapRow = { id: string; name: string; preview: string | null };
export type Board = {
  rank: number;
  playerId: string;
  displayName: string;
  rating: number;
  deviation: number;
  games: number;
  wins: number;
  losses: number;
  league: number;
  tier: number;
  hero: string | null;
};
export type Side = {
  slot: number;
  playerId: string;
  displayName: string;
  race: string;
  hero: string | null;
  ratingBefore: number;
  ratingAfter: number;
  won: boolean;
};
export type Match = {
  id: number;
  season: number;
  playedAt: string;
  map: string;
  duration: number;
  replay: string | null;
  sides: Side[];
};
export type RaceRating = {
  race: string;
  rating: number;
  deviation: number;
  games: number;
  wins: number;
  losses: number;
  league: number | null;
  tier: number | null;
  rank: number | null;
  hero: string | null;
};
export type Player = { id: number; playerId: string; displayName: string; createdAt: string };

type Row = Record<string, unknown>;
const num = (v: unknown) => Number(v);
const str = (v: unknown) => String(v);
const opt = (v: unknown) => (v === null || v === undefined ? null : String(v));

export class LadderDb {
  readonly db: DatabaseSync;

  constructor(file: string) {
    this.db = new DatabaseSync(file);
    this.db.exec("pragma journal_mode = wal; pragma foreign_keys = on; pragma busy_timeout = 2000;");
    this.db.exec(SCHEMA);
  }

  close(): void {
    this.db.close();
  }

  /** Runs `work` in one transaction. */
  transaction<T>(work: () => T): T {
    this.db.exec("begin");
    try {
      const out = work();
      this.db.exec("commit");
      return out;
    } catch (e) {
      this.db.exec("rollback");
      throw e;
    }
  }

  // ---- Writing ----

  addSeason(s: { number: number; name: string; startsAt: Date; endsAt: Date }): number {
    return Number(
      this.db.prepare("insert into seasons (number, name, starts_at, ends_at) values (?, ?, ?, ?)").run(s.number, s.name, s.startsAt.toISOString(), s.endsAt.toISOString()).lastInsertRowid,
    );
  }

  /** Content the pages name: races (in order), heroes, maps. */
  putRace(r: { id: string; name: string; description?: string; position: number; accent: string }): void {
    this.db
      .prepare("insert or replace into races (id, name, description, position, accent) values (?, ?, ?, ?, ?)")
      .run(r.id, r.name, r.description ?? "", r.position, r.accent);
  }

  putHero(h: Hero): void {
    this.db.prepare("insert or replace into heroes (id, race, name, portrait) values (?, ?, ?, ?)").run(h.id, h.race, h.name, h.portrait);
  }

  putMap(m: MapRow): void {
    this.db.prepare("insert or replace into maps (id, name, preview) values (?, ?, ?)").run(m.id, m.name, m.preview);
  }

  addPlayer(p: { playerId: string; displayName: string; createdAt: Date }): number {
    return Number(
      this.db.prepare("insert into players (player_id, display_name, created_at) values (?, ?, ?)").run(p.playerId, p.displayName, p.createdAt.toISOString()).lastInsertRowid,
    );
  }

  /** Records a ranked 1v1 and rates it: both players' race ratings move by Glicko-2 and
   * their league is set once placement is over. Returns the match's id. */
  recordMatch(m: {
    season: number;
    playedAt: Date;
    map: string;
    duration: number;
    replay: string | null;
    winner: 0 | 1;
    sides: [{ player: number; race: string; hero: string | null }, { player: number; race: string; hero: string | null }];
  }): number {
    const at = m.playedAt.toISOString();
    const current = m.sides.map((s) => {
      const row = this.db
        .prepare("select rating, deviation, volatility, games, wins, losses from ratings where player = ? and season = ? and race = ?")
        .get(s.player, m.season, s.race) as Row | undefined;
      return row
        ? { rating: num(row.rating), deviation: num(row.deviation), volatility: num(row.volatility), games: num(row.games), wins: num(row.wins), losses: num(row.losses) }
        : { ...INITIAL, games: 0, wins: 0, losses: 0 };
    });
    const w = current[m.winner]!;
    const l = current[1 - m.winner]!;
    const next = rate(w, l);
    const after = m.winner === 0 ? [next.winner, next.loser] : [next.loser, next.winner];
    const id = Number(
      this.db.prepare("insert into matches (season, played_at, map, duration, winner, replay) values (?, ?, ?, ?, ?, ?)").run(m.season, at, m.map, m.duration, m.winner, m.replay).lastInsertRowid,
    );
    m.sides.forEach((s, slot) => {
      const before = current[slot]!;
      const r = after[slot]!;
      const won = slot === m.winner;
      const games = before.games + 1;
      const st = standing(r.rating, games);
      this.db
        .prepare(
          `insert into ratings (player, season, race, rating, deviation, volatility, games, wins, losses, league, tier, updated_at)
           values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           on conflict (player, season, race) do update set rating = excluded.rating, deviation = excluded.deviation,
             volatility = excluded.volatility, games = excluded.games, wins = excluded.wins, losses = excluded.losses,
             league = excluded.league, tier = excluded.tier, updated_at = excluded.updated_at`,
        )
        .run(s.player, m.season, s.race, r.rating, r.deviation, r.volatility, games, before.wins + (won ? 1 : 0), before.losses + (won ? 0 : 1),
          st.placed ? st.league : null, st.placed ? st.tier : null, at);
      this.db
        .prepare("insert into match_players (match, slot, player, race, hero, rating_before, rating_after) values (?, ?, ?, ?, ?, ?, ?)")
        .run(id, slot, s.player, s.race, s.hero, before.rating, r.rating);
    });
    return id;
  }

  /** Empties every table. */
  clear(): void {
    for (const table of ["match_players", "matches", "ratings", "players", "heroes", "maps", "races", "seasons"]) this.db.exec(`delete from ${table}`);
  }

  // ---- Reading ----

  /** The season under way at `now`, or the latest one before it. */
  season(now: Date): Season | null {
    const row = this.db
      .prepare("select * from seasons where starts_at <= ? order by starts_at desc limit 1")
      .get(now.toISOString()) as Row | undefined;
    return row ? seasonRow(row) : null;
  }

  seasonByNumber(number: number): Season | null {
    const row = this.db.prepare("select * from seasons where number = ?").get(number) as Row | undefined;
    return row ? seasonRow(row) : null;
  }

  races(): Race[] {
    return (this.db.prepare("select * from races order by position").all() as Row[]).map((r) => ({
      id: str(r.id),
      name: str(r.name),
      description: str(r.description),
      accent: str(r.accent),
    }));
  }

  heroes(): Map<string, Hero> {
    const rows = this.db.prepare("select * from heroes").all() as Row[];
    return new Map(rows.map((r) => [str(r.id), { id: str(r.id), race: str(r.race), name: str(r.name), portrait: opt(r.portrait) }]));
  }

  maps(): Map<string, MapRow> {
    const rows = this.db.prepare("select * from maps").all() as Row[];
    return new Map(rows.map((r) => [str(r.id), { id: str(r.id), name: str(r.name), preview: opt(r.preview) }]));
  }

  /** Counts for the season's front page. */
  stats(season: number, now: Date): { players: number; matches: number; today: number; placed: number } {
    const day = new Date(now.getTime() - 24 * 3600 * 1000).toISOString();
    const one = (sql: string, ...args: (string | number)[]) => num((this.db.prepare(sql).get(...args) as Row).n);
    return {
      players: one("select count(distinct player) n from ratings where season = ? and games > 0", season),
      placed: one("select count(distinct player) n from ratings where season = ? and games >= ?", season, PLACEMENT_MATCHES),
      matches: one("select count(*) n from matches where season = ?", season),
      today: one("select count(*) n from matches where season = ? and played_at >= ?", season, day),
    };
  }

  /** A race's placed players, best first. */
  leaderboard(season: number, race: string, opts: { offset?: number; limit?: number; league?: number | null } = {}): { total: number; rows: Board[] } {
    const league = opts.league ?? null;
    const filter = league === null ? "" : "and league = :league";
    const params: Record<string, string | number> = { season, race, placed: PLACEMENT_MATCHES };
    if (league !== null) params.league = league;
    const total = num(
      (this.db.prepare(`select count(*) n from ratings where season = :season and race = :race and games >= :placed ${filter}`).get(params) as Row).n,
    );
    const rows = this.db
      .prepare(
        `with board as (
           select r.*, row_number() over (order by r.rating desc, r.player) as rank
           from ratings r where r.season = :season and r.race = :race and r.games >= :placed)
         select b.rank, p.player_id, p.display_name, b.rating, b.deviation, b.games, b.wins, b.losses, b.league, b.tier,
           (select mp.hero from match_players mp join matches m on m.id = mp.match
             where mp.player = b.player and mp.race = b.race and m.season = b.season
             group by mp.hero order by count(*) desc, mp.hero limit 1) as hero
         from board b join players p on p.id = b.player
         where 1 = 1 ${filter}
         order by b.rank limit :limit offset :offset`,
      )
      .all({ ...params, limit: opts.limit ?? 50, offset: opts.offset ?? 0 }) as Row[];
    return {
      total,
      rows: rows.map((r) => ({
        rank: num(r.rank),
        playerId: str(r.player_id),
        displayName: str(r.display_name),
        rating: num(r.rating),
        deviation: num(r.deviation),
        games: num(r.games),
        wins: num(r.wins),
        losses: num(r.losses),
        league: num(r.league),
        tier: num(r.tier),
        hero: opt(r.hero),
      })),
    };
  }

  /** Placed players per race and league: `counts[race][league]`. */
  distribution(season: number): Record<string, number[]> {
    const rows = this.db
      .prepare("select race, league, count(*) n from ratings where season = ? and league is not null group by race, league")
      .all(season) as Row[];
    const out: Record<string, number[]> = {};
    for (const r of rows) {
      const list = (out[str(r.race)] ??= []);
      list[num(r.league)] = num(r.n);
    }
    return out;
  }

  /** Each race's wins and games against other races (mirrors left out). */
  raceRecords(season: number): Record<string, { wins: number; games: number }> {
    const rows = this.db
      .prepare(
        `select a.race, sum(case when m.winner = a.slot then 1 else 0 end) wins, count(*) games
         from match_players a join match_players b on b.match = a.match and b.slot <> a.slot
         join matches m on m.id = a.match
         where m.season = ? and a.race <> b.race group by a.race`,
      )
      .all(season) as Row[];
    return Object.fromEntries(rows.map((r) => [str(r.race), { wins: num(r.wins), games: num(r.games) }]));
  }

  /** How often each hero is picked and wins this season. */
  heroRecords(season: number): { hero: string; race: string; games: number; wins: number }[] {
    return (
      this.db
        .prepare(
          `select mp.hero, mp.race, count(*) games, sum(case when m.winner = mp.slot then 1 else 0 end) wins
           from match_players mp join matches m on m.id = mp.match
           where m.season = ? and mp.hero is not null group by mp.hero, mp.race order by games desc`,
        )
        .all(season) as Row[]
    ).map((r) => ({ hero: str(r.hero), race: str(r.race), games: num(r.games), wins: num(r.wins) }));
  }

  player(playerId: string): Player | null {
    const r = this.db.prepare("select * from players where player_id = ?").get(playerId) as Row | undefined;
    return r ? { id: num(r.id), playerId: str(r.player_id), displayName: str(r.display_name), createdAt: str(r.created_at) } : null;
  }

  /** A player's ratings this season, with the rank on each placed race. */
  ratings(player: number, season: number): RaceRating[] {
    const rows = this.db
      .prepare(
        `select r.*,
           case when r.games >= :placed then
             (select count(*) + 1 from ratings o where o.season = r.season and o.race = r.race and o.games >= :placed
               and (o.rating > r.rating or (o.rating = r.rating and o.player < r.player))) end as rank,
           (select mp.hero from match_players mp join matches m on m.id = mp.match
             where mp.player = r.player and mp.race = r.race and m.season = r.season
             group by mp.hero order by count(*) desc, mp.hero limit 1) as hero
         from ratings r join races x on x.id = r.race
         where r.player = :player and r.season = :season order by x.position`,
      )
      .all({ player, season, placed: PLACEMENT_MATCHES }) as Row[];
    return rows.map((r) => ({
      race: str(r.race),
      rating: num(r.rating),
      deviation: num(r.deviation),
      games: num(r.games),
      wins: num(r.wins),
      losses: num(r.losses),
      league: r.league === null ? null : num(r.league),
      tier: r.tier === null ? null : num(r.tier),
      rank: r.rank === null ? null : num(r.rank),
      hero: opt(r.hero),
    }));
  }

  /** Matches, newest first: of one season, of one player, or of one race. */
  matches(opts: { season?: number; player?: number; race?: string; offset?: number; limit?: number }): { total: number; matches: Match[] } {
    const where: string[] = [];
    const params: Record<string, string | number> = {};
    if (opts.season !== undefined) {
      where.push("m.season = :season");
      params.season = opts.season;
    }
    if (opts.player !== undefined) {
      where.push("exists (select 1 from match_players x where x.match = m.id and x.player = :player)");
      params.player = opts.player;
    }
    if (opts.race !== undefined) {
      where.push("exists (select 1 from match_players x where x.match = m.id and x.race = :race)");
      params.race = opts.race;
    }
    const clause = where.length ? `where ${where.join(" and ")}` : "";
    const total = num((this.db.prepare(`select count(*) n from matches m ${clause}`).get(params) as Row).n);
    const rows = this.db
      .prepare(`select m.* from matches m ${clause} order by m.played_at desc, m.id desc limit :limit offset :offset`)
      .all({ ...params, limit: opts.limit ?? 25, offset: opts.offset ?? 0 }) as Row[];
    return { total, matches: this.withSides(rows) };
  }

  match(id: number): Match | null {
    const row = this.db.prepare("select * from matches where id = ?").get(id) as Row | undefined;
    return row ? this.withSides([row])[0]! : null;
  }

  /** Players whose Player ID or name contains `query`. */
  search(query: string, limit = 25): Player[] {
    const like = `%${query.replace(/[\\%_]/g, (c) => "\\" + c)}%`;
    return (
      this.db
        .prepare(
          `select * from players where player_id like :like escape '\\' or display_name like :like escape '\\'
           order by (player_id = :exact collate nocase) desc, length(player_id), player_id limit :limit`,
        )
        .all({ like, exact: query, limit }) as Row[]
    ).map((r) => ({ id: num(r.id), playerId: str(r.player_id), displayName: str(r.display_name), createdAt: str(r.created_at) }));
  }

  private withSides(rows: Row[]): Match[] {
    if (rows.length === 0) return [];
    const ids = rows.map((r) => num(r.id));
    const sides = this.db
      .prepare(
        `select mp.*, p.player_id, p.display_name from match_players mp join players p on p.id = mp.player
         where mp.match in (${ids.map(() => "?").join(",")}) order by mp.match, mp.slot`,
      )
      .all(...ids) as Row[];
    const bySide = new Map<number, Row[]>();
    for (const s of sides) {
      const list = bySide.get(num(s.match)) ?? [];
      list.push(s);
      bySide.set(num(s.match), list);
    }
    return rows.map((m) => ({
      id: num(m.id),
      season: num(m.season),
      playedAt: str(m.played_at),
      map: str(m.map),
      duration: num(m.duration),
      replay: opt(m.replay),
      sides: (bySide.get(num(m.id)) ?? []).map((s) => ({
        slot: num(s.slot),
        playerId: str(s.player_id),
        displayName: str(s.display_name),
        race: str(s.race),
        hero: opt(s.hero),
        ratingBefore: num(s.rating_before),
        ratingAfter: num(s.rating_after),
        won: num(s.slot) === num(m.winner),
      })),
    }));
  }
}

function seasonRow(r: Row): Season {
  return { id: num(r.id), number: num(r.number), name: str(r.name), startsAt: str(r.starts_at), endsAt: str(r.ends_at) };
}
