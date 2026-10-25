/**
 * The ladder's pages (docs/server.md#ladder): server-rendered HTML styled with Tailwind's
 * zinc palette. Accents are sparing: each race has its own (from the races table), leagues
 * have cool neutral colours, and nothing is green, olive or gold.
 */
import type { Board, Hero, Match, MapRow, Player, Race, RaceRating, Season, Side } from "./db";
import { Html, html, raw } from "./html";
import { LEAGUES, PLACEMENT_MATCHES, REPLAY_DAYS, SHOW_RATING, TIERS, replayKept, standingName } from "./leagues";

export type Context = {
  races: Race[];
  heroes: Map<string, Hero>;
  maps: Map<string, MapRow>;
  now: Date;
  /** Whether the site has a header image for this race. */
  hasRaceImage: (race: string) => boolean;
};

export const ROOT = "/ladder";

/** Accent classes by the races table's `accent`; written out in full so Tailwind finds them. */
const ACCENTS: Record<string, { text: string; dot: string; bar: string; chip: string }> = {
  rose: { text: "text-rose-300", dot: "bg-rose-400", bar: "bg-rose-400/80", chip: "bg-rose-400/10 text-rose-200 ring-rose-400/25" },
  violet: { text: "text-violet-300", dot: "bg-violet-400", bar: "bg-violet-400/80", chip: "bg-violet-400/10 text-violet-200 ring-violet-400/25" },
  sky: { text: "text-sky-300", dot: "bg-sky-400", bar: "bg-sky-400/80", chip: "bg-sky-400/10 text-sky-200 ring-sky-400/25" },
};
const accent = (race: Race | undefined) => ACCENTS[race?.accent ?? ""] ?? ACCENTS.sky!;

/** League colours, lowest to highest: cool neutrals rising to a pale violet. */
const LEAGUE_COLORS = ["#71717a", "#94a3b8", "#7dd3fc", "#a5b4fc", "#e9d5ff"];

// ---- Formatting --------------------------------------------------------------------------

const fmt = new Intl.NumberFormat("en-GB");
const n = (v: number) => fmt.format(v);
const pct = (part: number, whole: number) => (whole > 0 ? `${Math.round((part / whole) * 1000) / 10}%` : "–");
const day = (iso: string) => new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
const dayShort = (iso: string) => new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", timeZone: "UTC" });

function ago(iso: string, now: Date): string {
  const s = Math.max(0, (now.getTime() - new Date(iso).getTime()) / 1000);
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 7) return `${Math.round(s / 86400)} d ago`;
  return dayShort(iso);
}

function clock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, "0")}`;
}

function delta(side: Side): Html {
  const d = Math.round(side.ratingAfter - side.ratingBefore);
  if (!SHOW_RATING) return html``;
  return d >= 0
    ? html`<span class="tabular-nums text-xs text-indigo-300">+${d}</span>`
    : html`<span class="tabular-nums text-xs text-zinc-500">−${Math.abs(d)}</span>`;
}

const playerUrl = (id: string) => `${ROOT}/player/${encodeURIComponent(id)}`;
const raceUrl = (id: string) => `${ROOT}/race/${encodeURIComponent(id)}`;
const mediaUrl = (path: string) => `${ROOT}/media/${path}`;
const raceImage = (ctx: Context, race: string) => (ctx.hasRaceImage(race) ? `${ROOT}/static/img/race-${race}.webp` : `${ROOT}/static/img/hero.webp`);

// ---- Pieces ------------------------------------------------------------------------------

/** A hexagon in the league's colour with one pip per tier climbed; dashed while placing. */
export function badge(league: number | null, tier: number | null, size = 28): Html {
  if (league === null || tier === null) {
    return raw(
      `<svg width="${size}" height="${size}" viewBox="0 0 32 32" aria-hidden="true" class="shrink-0"><polygon points="16,2.5 28,9.25 28,22.75 16,29.5 4,22.75 4,9.25" fill="none" stroke="#52525b" stroke-width="1.5" stroke-dasharray="3 2.5"/><text x="16" y="20.5" text-anchor="middle" font-size="11" font-weight="600" fill="#71717a">?</text></svg>`,
    );
  }
  const c = LEAGUE_COLORS[league] ?? LEAGUE_COLORS[0]!;
  const pips = TIERS + 1 - tier;
  let dots = "";
  for (let i = 0; i < pips; i++) dots += `<circle cx="${16 + (i - (pips - 1) / 2) * 6}" cy="16" r="2.1" fill="${c}"/>`;
  const inner = league >= 3 ? `<polygon points="16,6.5 24.5,11.25 24.5,20.75 16,25.5 7.5,20.75 7.5,11.25" fill="none" stroke="${c}" stroke-opacity=".45" stroke-width="1"/>` : "";
  return raw(
    `<svg width="${size}" height="${size}" viewBox="0 0 32 32" aria-hidden="true" class="shrink-0"><polygon points="16,2.5 28,9.25 28,22.75 16,29.5 4,22.75 4,9.25" fill="${c}" fill-opacity=".14" stroke="${c}" stroke-width="1.5"/>${inner}${dots}</svg>`,
  );
}

function leagueLabel(league: number | null, tier: number | null, games: number): Html {
  const name = league === null || tier === null ? standingName({ placed: false, played: games }) : standingName({ placed: true, league, tier });
  const color = league === null ? "#a1a1aa" : LEAGUE_COLORS[league];
  return html`<span class="inline-flex items-center gap-2">${badge(league, tier, 22)}<span class="text-sm font-medium" style="color:${color}">${name}</span></span>`;
}

function rating(value: number, deviation?: number): Html {
  if (!SHOW_RATING) return html``;
  return html`<span class="tabular-nums font-semibold text-zinc-100">${Math.round(value)}</span>${
    deviation !== undefined ? html`<span class="ml-1 tabular-nums text-xs text-zinc-500">±${Math.round(deviation)}</span>` : ""
  }`;
}

function portrait(ctx: Context, hero: string | null, size = "size-7"): Html {
  const h = hero ? ctx.heroes.get(hero) : undefined;
  if (!h?.portrait) return html`<span class="${size} shrink-0 rounded-md bg-zinc-800 ring-1 ring-zinc-700"></span>`;
  return html`<img src="${mediaUrl(h.portrait)}" alt="${h.name}" title="${h.name}" loading="lazy" class="${size} shrink-0 rounded-md object-cover ring-1 ring-zinc-700">`;
}

function raceDot(ctx: Context, race: string): Html {
  const r = ctx.races.find((x) => x.id === race);
  return html`<span class="inline-flex items-center gap-1.5 text-xs ${accent(r).text}"><span class="size-1.5 rounded-full ${accent(r).dot}"></span>${r?.name ?? race}</span>`;
}

const DOWNLOAD_ICON = raw(
  `<svg class="size-3.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M10.75 2.75a.75.75 0 0 0-1.5 0v8.69L6.28 8.47a.75.75 0 0 0-1.06 1.06l4.25 4.25a.75.75 0 0 0 1.06 0l4.25-4.25a.75.75 0 1 0-1.06-1.06l-2.97 2.97V2.75Z"/><path d="M3.5 13.75a.75.75 0 0 0-1.5 0v2.5A1.75 1.75 0 0 0 3.75 18h12.5A1.75 1.75 0 0 0 18 16.25v-2.5a.75.75 0 0 0-1.5 0v2.5a.25.25 0 0 1-.25.25H3.75a.25.25 0 0 1-.25-.25v-2.5Z"/></svg>`,
);

function replayButton(ctx: Context, m: Match): Html {
  if (!m.replay) return html`<span class="text-xs text-zinc-600">No replay</span>`;
  if (!replayKept(new Date(m.playedAt), ctx.now)) return html`<span class="text-xs text-zinc-600" title="Replays are kept ${REPLAY_DAYS} days">Expired</span>`;
  const sample = m.replay === "sample";
  return html`<a href="${ROOT}/replays/${m.id}" download
    class="inline-flex items-center gap-1.5 rounded-md border border-zinc-700/80 bg-zinc-800/70 px-2.5 py-1 text-xs font-medium text-zinc-200 transition hover:border-zinc-500 hover:bg-zinc-700"
    title="${sample ? "Seeded match: downloads a labelled sample file, not a real replay" : "Download the replay (.utcr)"}">${DOWNLOAD_ICON}${sample ? "Sample" : "Replay"}</a>`;
}

function sideLine(ctx: Context, s: Side): Html {
  return html`<div class="flex min-w-0 items-center gap-2">
    ${portrait(ctx, s.hero)}
    <a href="${playerUrl(s.playerId)}" class="truncate text-sm ${s.won ? "font-semibold text-zinc-100" : "text-zinc-400"} hover:underline">${s.displayName}</a>
    ${s.won ? html`<span class="rounded bg-indigo-400/15 px-1.5 py-px text-[10px] font-semibold uppercase tracking-wide text-indigo-200">Win</span>` : ""}
    <span class="hidden sm:inline">${raceDot(ctx, s.race)}</span>
    ${delta(s)}
  </div>`;
}

function matchList(ctx: Context, matches: Match[], first?: string): Html {
  if (matches.length === 0) return empty("No matches yet", "Ranked matches show here as soon as they are played.");
  return html`<ul class="divide-y divide-zinc-800/80 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">
    ${matches.map((m) => {
      const map = ctx.maps.get(m.map);
      return html`<li class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-1 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_10rem_4rem_7.5rem]">
        <div class="min-w-0 space-y-1.5">${[...m.sides].sort((x, y) => Number(y.playerId === first) - Number(x.playerId === first)).map((s) => sideLine(ctx, s))}
          <p class="text-xs text-zinc-500 sm:hidden">${map?.name ?? m.map} · ${clock(m.duration)} · ${ago(m.playedAt, ctx.now)}</p>
        </div>
        <div class="hidden min-w-0 items-center gap-2 sm:flex">
          ${map?.preview ? html`<img src="${mediaUrl(map.preview)}" alt="" loading="lazy" class="size-8 shrink-0 rounded object-cover opacity-70 grayscale ring-1 ring-zinc-700">` : ""}
          <span class="truncate text-sm text-zinc-400">${map?.name ?? m.map}</span>
        </div>
        <div class="hidden text-sm tabular-nums text-zinc-400 sm:block">${clock(m.duration)}</div>
        <div class="flex flex-col items-end gap-1.5">
          <span class="hidden text-xs text-zinc-500 sm:block" title="${m.playedAt}">${ago(m.playedAt, ctx.now)}</span>
          ${replayButton(ctx, m)}
        </div>
      </li>`;
    })}
  </ul>`;
}

function empty(title: string, body: string): Html {
  return html`<div class="overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">
    <img src="${ROOT}/static/img/empty.webp" alt="" class="h-44 w-full object-cover opacity-70 sm:h-56">
    <div class="px-6 py-6 text-center">
      <p class="font-medium text-zinc-200">${title}</p>
      <p class="mt-1 text-sm text-zinc-500">${body}</p>
    </div>
  </div>`;
}

function sectionTitle(title: string, link?: { href: string; label: string }): Html {
  return html`<div class="mb-4 flex items-end justify-between gap-4">
    <h2 class="text-lg font-semibold tracking-tight text-zinc-100">${title}</h2>
    ${link ? html`<a href="${link.href}" class="text-sm text-zinc-400 hover:text-zinc-100">${link.label} →</a>` : ""}
  </div>`;
}

function pager(base: string, page: number, pages: number, query: Record<string, string | number | null> = {}): Html {
  if (pages <= 1) return html``;
  const href = (p: number) => {
    const q = new URLSearchParams();
    for (const [k, v] of Object.entries(query)) if (v !== null && v !== "") q.set(k, String(v));
    if (p > 1) q.set("page", String(p));
    const s = q.toString();
    return s ? `${base}?${s}` : base;
  };
  const btn = "rounded-md border border-zinc-800 px-3 py-1.5 text-sm";
  return html`<nav class="mt-4 flex items-center justify-between gap-2" aria-label="Pages">
    ${page > 1 ? html`<a class="${btn} text-zinc-300 hover:border-zinc-600 hover:text-zinc-100" href="${href(page - 1)}">← Previous</a>` : html`<span class="${btn} text-zinc-700">← Previous</span>`}
    <span class="text-sm tabular-nums text-zinc-500">Page ${page} of ${pages}</span>
    ${page < pages ? html`<a class="${btn} text-zinc-300 hover:border-zinc-600 hover:text-zinc-100" href="${href(page + 1)}">Next →</a>` : html`<span class="${btn} text-zinc-700">Next →</span>`}
  </nav>`;
}

function chips(items: { label: string; href: string; active: boolean }[]): Html {
  return html`<div class="-mx-4 flex gap-2 overflow-x-auto px-4 pb-1 sm:mx-0 sm:flex-wrap sm:px-0">
    ${items.map(
      (i) =>
        html`<a href="${i.href}" class="shrink-0 rounded-full px-3 py-1 text-sm ring-1 ${
          i.active ? "bg-zinc-100 text-zinc-900 ring-zinc-100" : "text-zinc-400 ring-zinc-800 hover:text-zinc-100 hover:ring-zinc-600"
        }">${i.label}</a>`,
    )}
  </div>`;
}

// ---- Layout ------------------------------------------------------------------------------

const LOGO = raw(
  `<svg class="size-6" viewBox="0 0 24 24" fill="none" aria-hidden="true"><path d="M12 2.5 20.5 7.25v9.5L12 21.5l-8.5-4.75v-9.5L12 2.5Z" stroke="#a1a1aa" stroke-width="1.4"/><path d="M12 6.5c2.6 2.2 3.4 5.2 0 11-3.4-5.8-2.6-8.8 0-11Z" fill="#e4e4e7"/><path d="M12 9v8.5" stroke="#18181b" stroke-width="1"/></svg>`,
);

export function layout(ctx: Context, opts: { title: string; active?: string; description?: string }, body: Html): string {
  const links = [
    { id: "overview", label: "Overview", href: ROOT },
    ...ctx.races.map((r) => ({ id: `race:${r.id}`, label: r.name, href: raceUrl(r.id) })),
    { id: "matches", label: "Matches", href: `${ROOT}/matches` },
  ];
  const nav = (cls: string) =>
    links.map(
      (l) =>
        html`<a href="${l.href}" class="${cls} rounded-md px-3 py-1.5 ${
          l.id === opts.active ? "bg-zinc-800 text-zinc-100" : "text-zinc-400 hover:text-zinc-100"
        }">${l.label}</a>`,
    );
  const page = html`<!doctype html>
<html lang="en" class="bg-zinc-950">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${opts.title} · Under the Canopy Ladder</title>
<meta name="description" content="${opts.description ?? "The ranked 1v1 ladder of Under the Canopy: leagues, leaderboards per race, players and replays."}">
<meta name="color-scheme" content="dark">
<link rel="stylesheet" href="${ROOT}/static/ladder.css">
<link rel="icon" href="data:image/svg+xml,${encodeURIComponent(LOGO.value.replace('class="size-6" ', ""))}">
</head>
<body class="min-h-screen bg-zinc-950 text-zinc-300 antialiased selection:bg-zinc-700">
<header class="sticky top-0 z-30 border-b border-zinc-800/80 bg-zinc-950/85 backdrop-blur">
  <div class="mx-auto flex h-14 max-w-6xl items-center gap-3 px-4">
    <a href="${ROOT}" class="flex shrink-0 items-center gap-2 font-semibold text-zinc-100">${LOGO}<span class="hidden sm:inline">Under the Canopy</span><span class="font-normal text-zinc-500">Ladder</span></a>
    <nav class="ml-4 hidden items-center gap-1 text-sm md:flex">${nav("")}</nav>
    <form action="${ROOT}/search" method="get" class="ml-auto" role="search">
      <label class="sr-only" for="q">Find a player</label>
      <input id="q" name="q" placeholder="Find a Player ID" autocomplete="off"
        class="w-40 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none sm:w-60">
    </form>
  </div>
  <nav class="flex gap-1 overflow-x-auto px-4 pb-2 text-sm md:hidden">${nav("shrink-0")}</nav>
</header>
<main>${body}</main>
<footer class="mt-20 border-t border-zinc-800/80">
  <div class="mx-auto flex max-w-6xl flex-wrap gap-x-6 gap-y-2 px-4 py-8 text-xs text-zinc-500">
    <span>Under the Canopy · Ranked 1v1 · Glicko-2 rating per race</span>
    <span>${PLACEMENT_MATCHES} placement matches, then leagues</span>
    <span>Replays kept ${REPLAY_DAYS} days</span>
    <a class="hover:text-zinc-300" href="${ROOT}/api/season">JSON API</a>
  </div>
</footer>
</body>
</html>`;
  return page.value;
}

function container(body: Html, cls = ""): Html {
  return html`<div class="mx-auto max-w-6xl px-4 ${cls}">${body}</div>`;
}

// ---- Pages -------------------------------------------------------------------------------

export type Overview = {
  season: Season;
  stats: { players: number; matches: number; today: number; placed: number };
  top: Record<string, Board[]>;
  distribution: Record<string, number[]>;
  raceRecords: Record<string, { wins: number; games: number }>;
  heroRecords: { hero: string; race: string; games: number; wins: number }[];
  recent: Match[];
};

export function overviewPage(ctx: Context, o: Overview | null): string {
  if (!o) {
    return layout(ctx, { title: "Ladder", active: "overview" }, container(html`<div class="py-16">${empty("No season yet", "The ladder opens with its first ranked season.")}</div>`));
  }
  const left = Math.max(0, Math.ceil((new Date(o.season.endsAt).getTime() - ctx.now.getTime()) / 86400000));
  const lastDay = new Date(new Date(o.season.endsAt).getTime() - 1).toISOString();
  const stat = (label: string, value: string) =>
    html`<div><dt class="text-xs uppercase tracking-wider text-zinc-500">${label}</dt><dd class="mt-1 text-2xl font-semibold tabular-nums text-zinc-100 sm:text-3xl">${value}</dd></div>`;

  const hero = html`<section class="relative overflow-hidden border-b border-zinc-800/80">
    <img src="${ROOT}/static/img/hero.webp" alt="" class="absolute inset-0 h-full w-full object-cover object-center opacity-80">
    <div class="absolute inset-0 bg-linear-to-t from-zinc-950 via-zinc-950/55 to-zinc-950/5"></div>
    <div class="absolute inset-0 bg-linear-to-r from-zinc-950/85 via-zinc-950/30 to-transparent"></div>
    <div class="relative mx-auto max-w-6xl px-4 pt-28 pb-10 sm:pt-44 sm:pb-14">
      <p class="text-xs font-medium uppercase tracking-[0.25em] text-zinc-400">Ranked 1v1 · ${o.season.name}</p>
      <h1 class="mt-3 text-4xl font-semibold tracking-tight text-white sm:text-6xl">The Ladder</h1>
      <p class="mt-3 max-w-xl text-zinc-300">${day(o.season.startsAt)} – ${day(lastDay)} · ${left} days left. One rating per race; ${PLACEMENT_MATCHES} placement matches, then a league.</p>
      <dl class="mt-8 grid max-w-lg grid-cols-3 gap-6">
        ${stat("Players", n(o.stats.players))}${stat("Matches", n(o.stats.matches))}${stat("Last 24 h", n(o.stats.today))}
      </dl>
    </div>
  </section>`;

  const best = html`<section class="mt-12">
    ${sectionTitle("Best players by race")}
    <div class="grid gap-6 md:grid-cols-2">
      ${ctx.races.map((race) => {
        const rows = o.top[race.id] ?? [];
        const rec = o.raceRecords[race.id];
        const placed = (o.distribution[race.id] ?? []).reduce((s, v) => s + (v ?? 0), 0);
        return html`<article class="overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">
          <a href="${raceUrl(race.id)}" class="group relative block h-32 sm:h-36">
            <img src="${raceImage(ctx, race.id)}" alt="" class="absolute inset-0 h-full w-full object-cover object-[75%_45%] opacity-80 transition group-hover:opacity-100">
            <div class="absolute inset-0 bg-linear-to-t from-zinc-900 via-zinc-900/40 to-transparent"></div>
            <div class="absolute inset-x-4 bottom-3 flex items-end justify-between gap-3">
              <h3 class="text-xl font-semibold text-white">${race.name}</h3>
              <p class="text-right text-xs text-zinc-400">${n(placed)} placed${rec ? html` · <span class="${accent(race).text}">${pct(rec.wins, rec.games)}</span> vs others` : ""}</p>
            </div>
          </a>
          ${rows.length === 0
            ? html`<p class="px-4 py-8 text-center text-sm text-zinc-500">Nobody has finished placement yet.</p>`
            : html`<ol class="divide-y divide-zinc-800/70">
            ${rows.map(
              (r) => html`<li class="flex items-center gap-3 px-4 py-2.5">
                <span class="w-5 text-right text-sm tabular-nums ${r.rank <= 3 ? "font-semibold text-zinc-100" : "text-zinc-500"}">${r.rank}</span>
                ${badge(r.league, r.tier, 24)}
                <div class="min-w-0 flex-1">
                  <a href="${playerUrl(r.playerId)}" class="block truncate text-sm font-medium text-zinc-100 hover:underline">${r.displayName}</a>
                  <p class="truncate text-xs text-zinc-500">${r.playerId} · ${standingName({ placed: true, league: r.league, tier: r.tier })}</p>
                </div>
                ${portrait(ctx, r.hero, "size-7 hidden sm:block")}
                <div class="w-20 text-right">${rating(r.rating)}<p class="text-xs tabular-nums text-zinc-500">${r.wins}–${r.losses}</p></div>
              </li>`,
            )}
          </ol>`}
          <a href="${raceUrl(race.id)}" class="block border-t border-zinc-800 px-4 py-3 text-sm text-zinc-400 hover:bg-zinc-800/40 hover:text-zinc-100">Full ${race.name} leaderboard →</a>
        </article>`;
      })}
    </div>
  </section>`;

  return layout(
    ctx,
    { title: "Ladder", active: "overview" },
    html`${hero}${container(html`${best}
      <section class="mt-14 grid gap-6 lg:grid-cols-5">
        <div class="lg:col-span-3">${sectionTitle("League distribution")}${distributionChart(ctx, o.distribution)}</div>
        <div class="lg:col-span-2">${sectionTitle("Heroes this season")}${heroTable(ctx, o.heroRecords)}</div>
      </section>
      <section class="mt-14">${sectionTitle("Recent matches", { href: `${ROOT}/matches`, label: "All matches" })}${matchList(ctx, o.recent)}</section>`)}`,
  );
}

function distributionChart(ctx: Context, dist: Record<string, number[]>): Html {
  const totals = LEAGUES.map((_, i) => ctx.races.reduce((s, r) => s + (dist[r.id]?.[i] ?? 0), 0));
  const all = totals.reduce((s, v) => s + v, 0);
  if (all === 0) return empty("No leagues yet", `Players join a league after ${PLACEMENT_MATCHES} placement matches.`);
  const max = Math.max(...totals);
  return html`<div class="rounded-xl border border-zinc-800 bg-zinc-900/50 p-4 sm:p-6">
    <div class="grid h-56 grid-cols-5 items-end gap-2 sm:gap-5">
      ${LEAGUES.map((league, i) => {
        const total = totals[i]!;
        return html`<div class="flex h-full flex-col items-center justify-end gap-2">
          <span class="text-sm font-medium tabular-nums text-zinc-200">${pct(total, all)}</span>
          <div class="flex w-full max-w-16 flex-col-reverse overflow-hidden rounded-md bg-zinc-800/60" style="height:${Math.max(2, (total / max) * 100)}%" title="${league.name}: ${n(total)} players">
            ${ctx.races.map((r) => {
              const v = dist[r.id]?.[i] ?? 0;
              return html`<div class="${accent(r).bar}" style="height:${total ? (v / total) * 100 : 0}%" title="${r.name}: ${n(v)}"></div>`;
            })}
          </div>
        </div>`;
      })}
    </div>
    <div class="mt-3 grid grid-cols-5 gap-2 sm:gap-5">
      ${LEAGUES.map(
        (league, i) => html`<div class="flex flex-col items-center gap-1 text-center">
          ${badge(i, 1, 22)}<span class="text-[11px] font-medium sm:text-xs" style="color:${LEAGUE_COLORS[i]}">${league.name}</span>
        </div>`,
      )}
    </div>
    <div class="mt-5 flex flex-wrap gap-4 border-t border-zinc-800 pt-4 text-xs text-zinc-400">
      ${ctx.races.map((r) => html`<span class="inline-flex items-center gap-1.5"><span class="size-2 rounded-sm ${accent(r).dot}"></span>${r.name}</span>`)}
      <span class="ml-auto text-zinc-500">${n(all)} placed race ratings</span>
    </div>
  </div>`;
}

function heroTable(ctx: Context, records: { hero: string; race: string; games: number; wins: number }[]): Html {
  if (records.length === 0) return empty("No heroes picked yet", "Hero picks show once matches are played.");
  const perRace: Record<string, number> = {};
  for (const r of records) perRace[r.race] = (perRace[r.race] ?? 0) + r.games;
  return html`<ul class="divide-y divide-zinc-800/70 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">
    ${records.map((r) => {
      const h = ctx.heroes.get(r.hero);
      const share = perRace[r.race] ? r.games / perRace[r.race]! : 0;
      const race = ctx.races.find((x) => x.id === r.race);
      return html`<li class="flex items-center gap-3 px-4 py-3">
        ${portrait(ctx, r.hero, "size-10")}
        <div class="min-w-0 flex-1">
          <p class="truncate text-sm font-medium text-zinc-100">${h?.name ?? r.hero}</p>
          <div class="mt-1.5 flex items-center gap-2">
            <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-zinc-800"><div class="h-full rounded-full ${accent(race).bar}" style="width:${share * 100}%"></div></div>
            <span class="w-10 text-right text-xs tabular-nums text-zinc-400">${pct(r.games, perRace[r.race] ?? 0)}</span>
          </div>
        </div>
        <div class="w-16 text-right"><p class="text-sm tabular-nums text-zinc-200">${pct(r.wins, r.games)}</p><p class="text-[11px] text-zinc-500">win rate</p></div>
      </li>`;
    })}
  </ul>`;
}

export type RaceBoard = {
  race: Race;
  season: Season | null;
  board: { total: number; rows: Board[] };
  page: number;
  pages: number;
  league: number | null;
  placed: number;
  distribution: number[];
  record: { wins: number; games: number } | undefined;
};

export function racePage(ctx: Context, b: RaceBoard): string {
  const a = accent(b.race);
  const top = html`<section class="relative overflow-hidden border-b border-zinc-800/80">
    <img src="${raceImage(ctx, b.race.id)}" alt="" class="absolute inset-0 h-full w-full object-cover object-[70%_50%] opacity-80">
    <div class="absolute inset-0 bg-linear-to-t from-zinc-950 via-zinc-950/50 to-transparent"></div>
    <div class="absolute inset-0 bg-linear-to-r from-zinc-950/90 via-zinc-950/40 to-transparent"></div>
    <div class="relative mx-auto max-w-6xl px-4 pt-24 pb-8 sm:pt-36 sm:pb-10">
      <p class="text-xs font-medium uppercase tracking-[0.25em] ${a.text}">${b.season?.name ?? "Ladder"} · Leaderboard</p>
      <h1 class="mt-2 text-4xl font-semibold tracking-tight text-white sm:text-5xl">${b.race.name}</h1>
      <p class="mt-3 max-w-xl text-sm text-zinc-300 sm:text-base">${b.race.description}</p>
      <dl class="mt-6 flex flex-wrap gap-x-8 gap-y-3 text-sm">
        <div><dt class="text-xs uppercase tracking-wider text-zinc-500">Placed</dt><dd class="mt-0.5 text-xl font-semibold tabular-nums text-zinc-100">${n(b.placed)}</dd></div>
        <div><dt class="text-xs uppercase tracking-wider text-zinc-500">Vs other races</dt><dd class="mt-0.5 text-xl font-semibold tabular-nums text-zinc-100">${b.record ? pct(b.record.wins, b.record.games) : "–"}</dd></div>
        <div><dt class="text-xs uppercase tracking-wider text-zinc-500">In ${LEAGUES[LEAGUES.length - 1]!.name}</dt><dd class="mt-0.5 text-xl font-semibold tabular-nums text-zinc-100">${n(b.distribution[LEAGUES.length - 1] ?? 0)}</dd></div>
      </dl>
    </div>
  </section>`;

  const filters = chips([
    { label: "All leagues", href: raceUrl(b.race.id), active: b.league === null },
    ...LEAGUES.map((l, i) => ({ label: `${l.name} · ${n(b.distribution[i] ?? 0)}`, href: `${raceUrl(b.race.id)}?league=${i}`, active: b.league === i })).reverse(),
  ]);

  const th = "px-4 py-2.5 text-left text-xs font-medium uppercase tracking-wider text-zinc-500";
  const table =
    b.board.rows.length === 0
      ? empty("Nobody here yet", b.league === null ? `Players appear after ${PLACEMENT_MATCHES} placement matches.` : "No player of this race is in this league.")
      : html`<div class="overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">
      <table class="w-full table-fixed">
        <thead class="border-b border-zinc-800 bg-zinc-900/80"><tr>
          <th class="${th} w-12 text-right sm:w-16">#</th>
          <th class="${th}">Player</th>
          <th class="${th} hidden w-44 md:table-cell">League</th>
          ${SHOW_RATING ? html`<th class="${th} w-20 text-right sm:w-28">MMR</th>` : ""}
          <th class="${th} hidden w-28 text-right sm:table-cell">Record</th>
          <th class="${th} hidden w-20 text-right sm:table-cell">Win %</th>
          <th class="${th} hidden w-16 lg:table-cell">Hero</th>
        </tr></thead>
        <tbody class="divide-y divide-zinc-800/70">
          ${b.board.rows.map(
            (r) => html`<tr class="hover:bg-zinc-800/30">
              <td class="px-4 py-2.5 text-right text-sm tabular-nums ${r.rank <= 3 ? "font-semibold text-zinc-100" : "text-zinc-500"}">${r.rank}</td>
              <td class="px-4 py-2.5"><div class="flex min-w-0 items-center gap-3">
                <span class="md:hidden">${badge(r.league, r.tier, 22)}</span>
                <div class="min-w-0"><a href="${playerUrl(r.playerId)}" class="block truncate text-sm font-medium text-zinc-100 hover:underline">${r.displayName}</a>
                <p class="truncate text-xs text-zinc-500">${r.playerId}<span class="sm:hidden"> · ${r.wins}–${r.losses}</span></p></div>
              </div></td>
              <td class="hidden px-4 py-2.5 md:table-cell">${leagueLabel(r.league, r.tier, r.games)}</td>
              ${SHOW_RATING ? html`<td class="px-4 py-2.5 text-right text-sm">${rating(r.rating)}</td>` : ""}
              <td class="hidden px-4 py-2.5 text-right text-sm tabular-nums text-zinc-400 sm:table-cell">${r.wins}–${r.losses}</td>
              <td class="hidden px-4 py-2.5 text-right text-sm tabular-nums text-zinc-300 sm:table-cell">${pct(r.wins, r.games)}</td>
              <td class="hidden px-4 py-2 lg:table-cell">${portrait(ctx, r.hero, "size-8")}</td>
            </tr>`,
          )}
        </tbody>
      </table>
    </div>`;

  return layout(
    ctx,
    { title: `${b.race.name} leaderboard`, active: `race:${b.race.id}` },
    html`${top}${container(html`<div class="mt-8 space-y-4">${filters}${table}${pager(raceUrl(b.race.id), b.page, b.pages, { league: b.league })}</div>`)}`,
  );
}

export type Profile = {
  player: Player;
  season: Season | null;
  ratings: RaceRating[];
  totals: Record<string, number>;
  matches: Match[];
  page: number;
  pages: number;
};

export function playerPage(ctx: Context, p: Profile): string {
  const wins = p.ratings.reduce((s, r) => s + r.wins, 0);
  const losses = p.ratings.reduce((s, r) => s + r.losses, 0);
  const initials = p.player.displayName.replace(/[^A-Za-z0-9 ]/g, "").split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase() || "?";
  const header = html`<section class="border-b border-zinc-800/80 bg-linear-to-b from-zinc-900 to-zinc-950">
    <div class="mx-auto flex max-w-6xl flex-col gap-5 px-4 py-10 sm:flex-row sm:items-center">
      <div class="flex size-16 shrink-0 items-center justify-center rounded-2xl bg-linear-to-br from-zinc-700 to-zinc-800 text-xl font-semibold text-zinc-100 ring-1 ring-zinc-600">${initials}</div>
      <div class="min-w-0">
        <h1 class="truncate text-3xl font-semibold tracking-tight text-white">${p.player.displayName}</h1>
        <p class="mt-1 text-sm text-zinc-400">Player ID <span class="font-mono text-zinc-200">${p.player.playerId}</span> · since ${day(p.player.createdAt)}</p>
      </div>
      <dl class="flex gap-8 sm:ml-auto">
        <div><dt class="text-xs uppercase tracking-wider text-zinc-500">Record</dt><dd class="mt-0.5 text-xl font-semibold tabular-nums text-zinc-100">${wins}–${losses}</dd></div>
        <div><dt class="text-xs uppercase tracking-wider text-zinc-500">Win rate</dt><dd class="mt-0.5 text-xl font-semibold tabular-nums text-zinc-100">${pct(wins, wins + losses)}</dd></div>
      </dl>
    </div>
  </section>`;

  const cards = html`<div class="grid gap-6 sm:grid-cols-2">
    ${ctx.races.map((race) => {
      const r = p.ratings.find((x) => x.race === race.id);
      const a = accent(race);
      const strip = html`<div class="relative h-20">
        <img src="${raceImage(ctx, race.id)}" alt="" class="absolute inset-0 h-full w-full object-cover object-[75%_45%] opacity-60">
        <div class="absolute inset-0 bg-linear-to-t from-zinc-900 to-zinc-900/20"></div>
        <h3 class="absolute bottom-2 left-4 text-lg font-semibold text-white">${race.name}</h3>
      </div>`;
      if (!r || r.games === 0) {
        return html`<article class="overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">${strip}
          <p class="px-4 py-8 text-center text-sm text-zinc-500">No ranked games as ${race.name} ${p.season ? `in ${p.season.name}` : ""}.</p></article>`;
      }
      const hero = r.hero ? ctx.heroes.get(r.hero) : undefined;
      return html`<article class="overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">${strip}
        <div class="flex items-center gap-4 px-4 pt-4">
          ${badge(r.league, r.tier, 52)}
          <div class="min-w-0">
            <p class="text-lg font-semibold" style="color:${r.league === null ? "#d4d4d8" : LEAGUE_COLORS[r.league]}">${r.league === null || r.tier === null ? standingName({ placed: false, played: r.games }) : standingName({ placed: true, league: r.league, tier: r.tier })}</p>
            <p class="text-sm text-zinc-400">${r.rank !== null ? html`Rank <span class="font-medium text-zinc-200">#${n(r.rank)}</span> of ${n(p.totals[race.id] ?? 0)}` : `${PLACEMENT_MATCHES - r.games} placement ${PLACEMENT_MATCHES - r.games === 1 ? "match" : "matches"} to go`}</p>
          </div>
        </div>
        <dl class="mt-4 grid grid-cols-3 gap-px border-t border-zinc-800 bg-zinc-800">
          ${SHOW_RATING ? html`<div class="bg-zinc-900 px-4 py-3"><dt class="text-[11px] uppercase tracking-wider text-zinc-500">MMR</dt><dd class="mt-0.5 text-sm">${r.league === null ? html`<span class="text-zinc-500" title="Shown once placed">Placing</span>` : rating(r.rating, r.deviation)}</dd></div>` : ""}
          <div class="bg-zinc-900 px-4 py-3"><dt class="text-[11px] uppercase tracking-wider text-zinc-500">Record</dt><dd class="mt-0.5 text-sm tabular-nums text-zinc-200">${r.wins}–${r.losses}</dd></div>
          <div class="bg-zinc-900 px-4 py-3"><dt class="text-[11px] uppercase tracking-wider text-zinc-500">Win %</dt><dd class="mt-0.5 text-sm tabular-nums ${a.text}">${pct(r.wins, r.games)}</dd></div>
        </dl>
        ${hero ? html`<div class="flex items-center gap-3 border-t border-zinc-800 px-4 py-3">${portrait(ctx, hero.id, "size-9")}<div><p class="text-[11px] uppercase tracking-wider text-zinc-500">Most played hero</p><p class="text-sm text-zinc-200">${hero.name}</p></div></div>` : ""}
      </article>`;
    })}
  </div>`;

  return layout(
    ctx,
    { title: p.player.displayName, description: `${p.player.displayName} (${p.player.playerId}) on the Under the Canopy ladder.` },
    html`${header}${container(html`<section class="mt-8">${sectionTitle(p.season ? `${p.season.name} ratings` : "Ratings")}${cards}</section>
      <section class="mt-12">${sectionTitle("Recent matches")}${matchList(ctx, p.matches, p.player.playerId)}${pager(playerUrl(p.player.playerId), p.page, p.pages)}</section>`)}`,
  );
}

export function matchesPage(ctx: Context, m: { matches: Match[]; total: number; page: number; pages: number; race: string | null; season: Season | null }): string {
  const filters = chips([
    { label: "All races", href: `${ROOT}/matches`, active: m.race === null },
    ...ctx.races.map((r) => ({ label: r.name, href: `${ROOT}/matches?race=${encodeURIComponent(r.id)}`, active: m.race === r.id })),
  ]);
  return layout(
    ctx,
    { title: "Matches", active: "matches" },
    container(
      html`<div class="py-10">
        <p class="text-xs font-medium uppercase tracking-[0.25em] text-zinc-500">${m.season?.name ?? "Ladder"}</p>
        <h1 class="mt-2 text-3xl font-semibold tracking-tight text-white">Recent matches</h1>
        <p class="mt-2 text-sm text-zinc-400">${n(m.total)} ranked matches. Every replay stays downloadable for ${REPLAY_DAYS} days.</p>
        <div class="mt-6 space-y-4">${filters}${matchList(ctx, m.matches)}${pager(`${ROOT}/matches`, m.page, m.pages, { race: m.race })}</div>
      </div>`,
    ),
  );
}

export function searchPage(ctx: Context, query: string, players: Player[]): string {
  return layout(
    ctx,
    { title: "Find a player" },
    container(
      html`<div class="py-10">
        <h1 class="text-3xl font-semibold tracking-tight text-white">Find a player</h1>
        <form action="${ROOT}/search" class="mt-6 flex gap-2">
          <input name="q" value="${query}" placeholder="Player ID or name" class="min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2 text-zinc-100 placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none">
          <button class="rounded-md bg-zinc-100 px-4 py-2 text-sm font-medium text-zinc-900 hover:bg-white">Search</button>
        </form>
        <div class="mt-6">
          ${query && players.length === 0
            ? empty("No player found", `Nobody's Player ID or name contains “${query}”.`)
            : html`<ul class="divide-y divide-zinc-800/70 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/50">${players.map(
                (p) => html`<li><a href="${playerUrl(p.playerId)}" class="flex items-center justify-between gap-4 px-4 py-3 hover:bg-zinc-800/40">
                  <span class="truncate text-sm font-medium text-zinc-100">${p.displayName}</span><span class="font-mono text-xs text-zinc-500">${p.playerId}</span></a></li>`,
              )}</ul>`}
        </div>
      </div>`,
    ),
  );
}

export function notFoundPage(ctx: Context, what = "This page"): string {
  return layout(
    ctx,
    { title: "Not found" },
    container(html`<div class="mx-auto max-w-xl py-16">${empty(`${what} isn't here`, "It may have moved, or it never grew.")}
      <p class="mt-6 text-center"><a href="${ROOT}" class="text-sm text-zinc-400 hover:text-zinc-100">← Back to the ladder</a></p></div>`),
  );
}
