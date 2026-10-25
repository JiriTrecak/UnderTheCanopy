/**
 * The ladder's rules in one place (D107): placement, leagues and tiers, seasons and how long
 * replays are kept. The league names are placeholders until the forest-themed ones are
 * settled: rename them here and nowhere else (the database stores a league's index).
 */

/** Lowest to highest. `from` is the rating a league starts at. */
export const LEAGUES = [
  { name: "Seedling", from: -Infinity },
  { name: "Sapling", from: 1300 },
  { name: "Thicket", from: 1500 },
  { name: "Elder", from: 1700 },
  { name: "Canopy", from: 1900 },
] as const;

/** Each league has three tiers; tier 1 is the highest, as in StarCraft II. */
export const TIERS = 3;
/** The rating a tier spans in the open-ended top and bottom leagues. */
const OPEN_TIER_SPAN = 200 / TIERS;
/** Ranked games a player plays on a race before that race is placed in a league. */
export const PLACEMENT_MATCHES = 5;
/** A season lasts three months. */
export const SEASON_MONTHS = 3;
/** The server keeps each online match's replay this long. */
export const REPLAY_DAYS = 90;
/** Whether the website shows ratings as numbers (MMR). The game never does (D107); the
 * site does because it is the ladder's MMR page. False hides every number and keeps the order. */
export const SHOW_RATING = true;

/** Where a race of a player stands: a league and tier, or still in placement. */
export type Standing =
  | { placed: true; league: number; tier: number }
  | { placed: false; played: number };

/** The league of a rating: an index into `LEAGUES`. */
export function leagueOf(rating: number): number {
  let league = 0;
  for (let i = 1; i < LEAGUES.length; i++) if (rating >= LEAGUES[i]!.from) league = i;
  return league;
}

/** The tier (1 highest … `TIERS` lowest) of a rating within its league. */
export function tierOf(rating: number): number {
  const league = leagueOf(rating);
  const top = league === LEAGUES.length - 1;
  const from = league === 0 ? LEAGUES[1]!.from - 200 : LEAGUES[league]!.from;
  const span = top || league === 0 ? OPEN_TIER_SPAN : (LEAGUES[league + 1]!.from - from) / TIERS;
  const step = Math.floor((rating - from) / span);
  return Math.min(TIERS, Math.max(1, TIERS - step));
}

/** A race's standing after `games` ranked games at this rating. */
export function standing(rating: number, games: number): Standing {
  if (games < PLACEMENT_MATCHES) return { placed: false, played: games };
  return { placed: true, league: leagueOf(rating), tier: tierOf(rating) };
}

const ROMAN = ["", "I", "II", "III", "IV", "V"];

/** "Thicket II", or "Placement 3/5". */
export function standingName(s: Standing): string {
  if (!s.placed) return `Placement ${s.played}/${PLACEMENT_MATCHES}`;
  return `${LEAGUES[s.league]!.name} ${ROMAN[s.tier] ?? s.tier}`;
}

/** The season that holds `date`: seasons start on the first of January, April, July and
 * October. Its number counts from the first ladder season. */
export function seasonOf(date: Date, first = { year: 2026, month: 9 }): { number: number; starts: Date; ends: Date } {
  const months = (date.getUTCFullYear() - first.year) * 12 + (date.getUTCMonth() - first.month);
  const index = Math.floor(months / SEASON_MONTHS);
  const startMonth = first.month + index * SEASON_MONTHS;
  const starts = new Date(Date.UTC(first.year, startMonth, 1));
  const ends = new Date(Date.UTC(first.year, startMonth + SEASON_MONTHS, 1));
  return { number: index + 1, starts, ends };
}

/** Whether a replay recorded at `playedAt` is still kept at `now`. */
export function replayKept(playedAt: Date, now: Date): boolean {
  return now.getTime() - playedAt.getTime() < REPLAY_DAYS * 24 * 3600 * 1000;
}
