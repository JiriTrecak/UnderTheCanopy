// The ladder's maths (D107): Glicko-2 as Glickman specifies it, leagues and tiers from a
// rating, placement, seasons and how long replays are kept.
import assert from "node:assert/strict";
import { test } from "node:test";
import { INITIAL, MAX_DEVIATION, match, softReset, update, winChance } from "../src/ladder/glicko";
import { LEAGUES, PLACEMENT_MATCHES, TIERS, leagueOf, replayKept, seasonOf, standing, standingName, tierOf } from "../src/ladder/leagues";

const close = (actual: number, expected: number, within: number) =>
  assert.ok(Math.abs(actual - expected) <= within, `${actual} is not within ${within} of ${expected}`);

test("Glicko-2 reproduces the worked example of Glickman's paper", () => {
  const next = update({ rating: 1500, deviation: 200, volatility: 0.06 }, [
    { opponent: { rating: 1400, deviation: 30, volatility: 0.06 }, score: 1 },
    { opponent: { rating: 1550, deviation: 100, volatility: 0.06 }, score: 0 },
    { opponent: { rating: 1700, deviation: 300, volatility: 0.06 }, score: 0 },
  ]);
  close(next.rating, 1464.06, 0.01);
  close(next.deviation, 151.52, 0.01);
  close(next.volatility, 0.05999, 0.00001);
});

test("a period without games only widens the deviation, never past a new player's", () => {
  const settled = { rating: 1720, deviation: 60, volatility: 0.06 };
  const idle = update(settled, []);
  assert.equal(idle.rating, 1720);
  assert.ok(idle.deviation > 60);
  let r = INITIAL;
  for (let i = 0; i < 50; i++) r = update(r, []);
  assert.equal(r.deviation, MAX_DEVIATION);
});

test("a win moves the winner up and the loser down; an upset moves them further", () => {
  const strong = { rating: 1800, deviation: 80, volatility: 0.06 };
  const weak = { rating: 1400, deviation: 80, volatility: 0.06 };
  const expected = match(strong, weak);
  const upset = match(weak, strong);
  assert.ok(expected.winner.rating > strong.rating && expected.loser.rating < weak.rating);
  assert.ok(upset.winner.rating - weak.rating > expected.winner.rating - strong.rating);
  assert.ok(expected.winner.deviation < strong.deviation);
});

test("the win chance favours the higher rating and is symmetric", () => {
  const a = { rating: 1700, deviation: 50, volatility: 0.06 };
  const b = { rating: 1500, deviation: 50, volatility: 0.06 };
  assert.ok(winChance(a, b) > 0.7);
  close(winChance(a, b) + winChance(b, a), 1, 1e-12);
  close(winChance(a, a), 0.5, 1e-12);
});

test("a soft reset pulls ratings towards the middle and makes them uncertain again", () => {
  const reset = softReset({ rating: 2100, deviation: 50, volatility: 0.06 });
  assert.ok(reset.rating < 2100 && reset.rating > 1500);
  assert.ok(reset.deviation > 50);
});

test("leagues rise with the rating and tiers count down to 1 within each league", () => {
  let last = { league: 0, tier: TIERS };
  for (let rating = 600; rating <= 2600; rating += 5) {
    const league = leagueOf(rating);
    const tier = tierOf(rating);
    assert.ok(tier >= 1 && tier <= TIERS);
    assert.ok(league > last.league || (league === last.league && tier <= last.tier), `rating ${rating} went down`);
    last = { league, tier };
  }
  assert.equal(leagueOf(-1e9), 0);
  assert.equal(leagueOf(1e9), LEAGUES.length - 1);
  for (let i = 1; i < LEAGUES.length; i++) {
    assert.equal(leagueOf(LEAGUES[i]!.from), i);
    assert.equal(tierOf(LEAGUES[i]!.from), TIERS, "a league starts at its lowest tier");
  }
});

test("a race stays in placement for its first matches", () => {
  for (let games = 0; games < PLACEMENT_MATCHES; games++) assert.deepEqual(standing(2400, games), { placed: false, played: games });
  const placed = standing(2400, PLACEMENT_MATCHES);
  assert.ok(placed.placed && placed.league === LEAGUES.length - 1 && placed.tier === 1);
  assert.equal(standingName({ placed: false, played: 2 }), `Placement 2/${PLACEMENT_MATCHES}`);
  assert.equal(standingName({ placed: true, league: 0, tier: 2 }), `${LEAGUES[0]!.name} II`);
});

test("seasons last three months and replays ninety days", () => {
  const first = seasonOf(new Date("2026-10-25T12:00:00Z"));
  assert.equal(first.number, 1);
  assert.equal(first.starts.toISOString(), "2026-10-01T00:00:00.000Z");
  assert.equal(first.ends.toISOString(), "2027-01-01T00:00:00.000Z");
  const next = seasonOf(new Date("2027-01-01T00:00:00Z"));
  assert.equal(next.number, 2);
  assert.equal(next.starts.toISOString(), first.ends.toISOString());
  const played = new Date("2026-10-01T00:00:00Z");
  assert.ok(replayKept(played, new Date("2026-12-29T23:00:00Z")));
  assert.ok(!replayKept(played, new Date("2026-12-30T00:00:00Z")));
});
