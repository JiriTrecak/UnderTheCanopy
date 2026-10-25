/**
 * Glicko-2 (Mark Glickman, "Example of the Glicko-2 system", 2012), the ladder's rating
 * maths (D107). Every ranked match is its own rating period: each player's rating is updated
 * from that one game against the opponent's rating before it.
 */

/** A player's rating on the Glicko-1 scale the ladder stores and shows. */
export type Rating = { rating: number; deviation: number; volatility: number };

/** One game against an opponent: `score` 1 win, 0 loss, 0.5 draw. */
export type Game = { opponent: Rating; score: number };

/** Where every player starts. */
export const INITIAL: Rating = { rating: 1500, deviation: 350, volatility: 0.06 };
/** How much the volatility may change per period (Glickman suggests 0.3 to 1.2). */
export const TAU = 0.5;
/** No deviation grows past a new player's. */
export const MAX_DEVIATION = 350;
/** The deviation a season's soft reset raises everyone to (at least). */
export const RESET_DEVIATION = 200;
/** How much of the distance from 1500 a soft reset keeps. */
export const RESET_KEEP = 0.75;

const SCALE = 173.7178;
const EPSILON = 0.000001;

function g(phi: number): number {
  return 1 / Math.sqrt(1 + (3 * phi * phi) / (Math.PI * Math.PI));
}

function expected(mu: number, muJ: number, phiJ: number): number {
  return 1 / (1 + Math.exp(-g(phiJ) * (mu - muJ)));
}

/** The player's rating after a period with these games; no games only widens the deviation. */
export function update(player: Rating, games: readonly Game[], tau = TAU): Rating {
  const mu = (player.rating - 1500) / SCALE;
  const phi = player.deviation / SCALE;
  const sigma = player.volatility;
  if (games.length === 0) {
    const widened = Math.sqrt(phi * phi + sigma * sigma) * SCALE;
    return { rating: player.rating, deviation: Math.min(widened, MAX_DEVIATION), volatility: sigma };
  }
  let vInverse = 0;
  let sum = 0;
  for (const game of games) {
    const muJ = (game.opponent.rating - 1500) / SCALE;
    const phiJ = game.opponent.deviation / SCALE;
    const e = expected(mu, muJ, phiJ);
    const gj = g(phiJ);
    vInverse += gj * gj * e * (1 - e);
    sum += gj * (game.score - e);
  }
  const v = 1 / vInverse;
  const delta = v * sum;
  const sigmaNext = volatility(delta, phi, v, sigma, tau);
  const phiStar = Math.sqrt(phi * phi + sigmaNext * sigmaNext);
  const phiNext = 1 / Math.sqrt(1 / (phiStar * phiStar) + 1 / v);
  const muNext = mu + phiNext * phiNext * sum;
  return {
    rating: muNext * SCALE + 1500,
    deviation: Math.min(phiNext * SCALE, MAX_DEVIATION),
    volatility: sigmaNext,
  };
}

/** Step 5 of the paper: the new volatility by the Illinois algorithm. */
function volatility(delta: number, phi: number, v: number, sigma: number, tau: number): number {
  const a = Math.log(sigma * sigma);
  const f = (x: number) => {
    const ex = Math.exp(x);
    const d = phi * phi + v + ex;
    return (ex * (delta * delta - phi * phi - v - ex)) / (2 * d * d) - (x - a) / (tau * tau);
  };
  let A = a;
  let B: number;
  if (delta * delta > phi * phi + v) {
    B = Math.log(delta * delta - phi * phi - v);
  } else {
    let k = 1;
    while (f(a - k * tau) < 0) k += 1;
    B = a - k * tau;
  }
  let fA = f(A);
  let fB = f(B);
  while (Math.abs(B - A) > EPSILON) {
    const C = A + ((A - B) * fA) / (fB - fA);
    const fC = f(C);
    if (fC * fB <= 0) {
      A = B;
      fA = fB;
    } else {
      fA /= 2;
    }
    B = C;
    fB = fC;
  }
  return Math.exp(A / 2);
}

/** One ranked 1v1: both players' ratings after it. */
export function match(winner: Rating, loser: Rating): { winner: Rating; loser: Rating } {
  return {
    winner: update(winner, [{ opponent: loser, score: 1 }]),
    loser: update(loser, [{ opponent: winner, score: 0 }]),
  };
}

/** The chance that `a` beats `b`, from both ratings and deviations. */
export function winChance(a: Rating, b: Rating): number {
  const phi = Math.sqrt(a.deviation * a.deviation + b.deviation * b.deviation) / SCALE;
  return 1 / (1 + Math.exp(-g(phi) * ((a.rating - b.rating) / SCALE)));
}

/** A new season's soft reset: ratings move a quarter of the way back to 1500 and become
 * less certain, so a few games settle each player again. */
export function softReset(player: Rating): Rating {
  return {
    rating: 1500 + (player.rating - 1500) * RESET_KEEP,
    deviation: Math.max(player.deviation, RESET_DEVIATION),
    volatility: player.volatility,
  };
}
