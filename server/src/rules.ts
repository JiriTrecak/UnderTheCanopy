/**
 * The parts of `content/game.json` the lobby checks: which races exist and which starting
 * heroes each may pick (old `raceDefinition` and `chosenHero`). Read once at start; a new
 * race or hero needs the server redeployed with the new file.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import type { HeroChoice } from "./protocol";

const id = z.string().regex(/^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/);
// Loose: the room view passes the authored object on, as the old server did.
const hero = z.looseObject({ default: id, choices: z.array(id).min(1).max(32) });
const setup = z.object({ hero: hero.optional() });
const rulesSchema = z.object({
  defaultRace: z.string(),
  startingSetup: setup,
  races: z.record(z.string(), z.object({ name: z.string().min(1).max(80), startingSetup: setup.optional() })),
});

export type Rules = z.infer<typeof rulesSchema>;

/** `game.json` beside the repository's server folder, or `GAME_JSON`. */
export function loadRules(path = process.env.GAME_JSON ?? fileURLToPath(new URL("../../content/game.json", import.meta.url))): Rules {
  const game = JSON.parse(readFileSync(path, "utf8")) as { rules?: unknown };
  return rulesSchema.parse(game.rules);
}

/** A race's display name and hero choice; the default race uses `defaultHeroes`. */
export function race(rules: Rules, raceId: string, defaultHeroes: HeroChoice | undefined): { name: string; heroes: HeroChoice | undefined } {
  const r = rules.races[raceId];
  if (!r && (raceId !== rules.defaultRace || Object.keys(rules.races).length)) throw Error(`Unknown race: ${raceId}`);
  if (raceId === rules.defaultRace) return { name: r?.name ?? raceId, heroes: defaultHeroes };
  return { name: r?.name ?? raceId, heroes: (r?.startingSetup ?? rules.startingSetup).hero };
}

/** The hero a slot starts with: `selected` or the default, if the policy offers it. */
export function chosenHero(policy: HeroChoice | undefined, selected?: string): string | undefined {
  if (!policy) {
    if (selected !== undefined) throw new Error("This setup has no starting hero choice");
    return;
  }
  const choice = selected ?? policy.default;
  if (!policy.choices.includes(choice)) throw new Error(`Unavailable starting hero: ${choice}`);
  return choice;
}
