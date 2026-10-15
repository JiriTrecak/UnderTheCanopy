/**
 * The frozen lobby and lockstep protocol (D10): wire messages, the match config, the
 * client-writable action shapes, chat clean-up and the parts of a multiplayer save the
 * host reads. Copied from the old game's `src/shared/{net,match,types,chat,save}` with
 * everything the server does not use left out; the game client in `crates/net` mirrors it.
 */
import { z } from "zod";

export const COMMAND_DELAY = 8;
export const CHECKSUM_EVERY = 8;
export const TICK_MS = 25;

export type Slot = {
  player: number;
  kind: "human" | "ai";
  name?: string;
  team?: number;
  hero?: string;
  race?: string;
};

export type MatchConfig = {
  v: 1;
  roomId: string;
  mapId: string;
  mapRevision: string;
  seed: number;
  delay: number;
  checksumEvery: number;
  tickMs: typeof TICK_MS;
  slots: Slot[];
  /** A campaign company; opaque to the server. */
  company?: unknown[];
};

const idSchema = z.string().regex(/^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/);
const pointSchema = z
  .object({
    x: z.number().int().min(0).max(2047),
    y: z.number().int().min(0).max(2047),
    surface: z.string().min(1).max(160).optional(),
  })
  .strict();
/** Static foundations may be centred between navigation cells. */
const placementPointSchema = pointSchema.extend({
  x: z.number().multipleOf(0.5).min(0).max(2047),
  y: z.number().multipleOf(0.5).min(0).max(2047),
});
const actor = z.number().int().positive();
const actors = z
  .array(actor)
  .min(1)
  .max(160)
  .refine((xs) => new Set(xs).size === xs.length, "Duplicate actors");

/** The only client-writable gameplay intentions. */
export const actionSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("abilityAutocast"), actor, binding: z.string().max(64), enabled: z.boolean() }).strict(),
  z.object({
    type: z.literal("castAbility"),
    actor,
    binding: z.string().regex(/^[a-z][a-z0-9-]*$/).max(64),
    target: z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("unit"), entity: actor }).strict(),
      z.object({ kind: z.literal("point"), position: z.object({ x: pointSchema.shape.x, y: pointSchema.shape.y }).strict() }).strict(),
    ]),
  }).strict(),
  z.object({ type: z.literal("garrison"), actors, target: actor, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("unload"), actor }).strict(),
  z.object({ type: z.literal("research"), actor, research: idSchema }).strict(),
  z.object({ type: z.literal("cancelResearch"), actor, research: idSchema }).strict(),
  z.object({ type: z.literal("upgrade"), actor }).strict(),
  z.object({ type: z.literal("cancelUpgrade"), actor }).strict(),
  z.object({ type: z.literal("revive"), actor, hero: actor }).strict(),
  z.object({ type: z.literal("cancelRevival"), actor, hero: actor }).strict(),
  z.object({ type: z.literal("learnAbility"), actor: z.number().int().positive(), ability: z.string().min(1) }).strict(),
  z.object({ type: z.literal("gather"), actors, target: actor, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("construct"), actors, target: actor, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("pickup"), actor, target: actor, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("dropItem"), actor, slot: z.number().int().min(0).max(11) }).strict(),
  z.object({ type: z.literal("useItem"), actor, slot: z.number().int().min(0).max(11) }).strict(),
  z.object({ type: z.literal("move"), actors, destination: pointSchema, attackMove: z.boolean().optional(), append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("attack"), actors, target: actor, force: z.boolean().optional(), append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("stop"), actors }).strict(),
  z.object({ type: z.literal("hold"), actors, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("patrol"), actors, destination: pointSchema, append: z.boolean().optional() }).strict(),
  z.object({ type: z.literal("follow"), actors, target: actor, append: z.boolean().optional() }).strict(),
  z.object({
    type: z.literal("build"),
    actors,
    definition: idSchema,
    position: placementPointSchema,
    rotation: z.number().int().multipleOf(90).optional(),
    append: z.boolean().optional(),
  }).strict(),
  z.object({ type: z.literal("produce"), actor, definition: idSchema }).strict(),
  z.object({ type: z.literal("cancel"), actor, queue: actor.optional() }).strict(),
  z.object({ type: z.literal("rally"), actor, destination: pointSchema.nullable(), target: actor.optional() }).strict(),
  z.object({ type: z.literal("pause"), actor, paused: z.boolean() }).strict(),
  z.object({ type: z.literal("noop") }).strict(),
  z.object({ type: z.literal("ping") }).strict(),
]);
export type Action = z.infer<typeof actionSchema>;
export const validAction = (value: unknown): value is Action => actionSchema.safeParse(value).success;

export type WireOutcome = { winner: number | null; defeated: number[] };
export type Bundle = { tick: number; actions: Action[] };
export type CommitSlot = { player: number; actions: Action[] };
export type Commit = { tick: number; slots: CommitSlot[] };
export type ClientIdentity = { role: "player" | "spectator"; player?: number; name: string };
export type RoomState = "waiting" | "playing" | "ended" | "desynced";
export type HeroChoice = { default: string; choices: string[] };

export type RoomView = {
  id: string;
  state: RoomState;
  name: string;
  mapId: string;
  host: string;
  slots: { player: number; name: string | null; roundTripMs?: number; hero?: string; race?: string }[];
  heroes?: HeroChoice;
  races?: Record<string, { name: string; heroes?: HeroChoice }>;
  spectators: number;
  tick?: number;
  inputDelayMs?: number;
};

export type CreateRoom = { name: string; mapId: string; mapRevision: string; slotCount: number; guestName: string };
export type JoinRoom = { guestName: string; role: "player" | "spectator" };
export type ChatMessage = { name: string; player: number | null; text: string };

export type ClientMsg =
  | { type: "latencyReply"; id: string }
  | { type: "chat"; text: string }
  | { type: "selectHero"; hero: string }
  | { type: "selectRace"; race: string }
  | { type: "startMatch" }
  | { type: "hello"; token: string }
  | { type: "ready"; content: { abi: string; sha256: string }; build: string }
  | { type: "turn"; through: number; bundles: Bundle[] }
  | { type: "hash"; tick: number; checksum: number }
  | { type: "ended"; outcome: WireOutcome; tick: number; checksum: number }
  | { type: "loadSave"; save: unknown }
  | { type: "restart" };

export type ServerMsg =
  | { type: "latencyProbe"; id: string }
  | { type: "chat"; message: ChatMessage }
  | { type: "welcome"; you: ClientIdentity; room: RoomView }
  | { type: "room"; room: RoomView }
  | { type: "start"; config: MatchConfig; you: ClientIdentity; save?: unknown }
  | { type: "restart"; config: MatchConfig; you: ClientIdentity }
  | { type: "go"; tick: number }
  | { type: "commit"; tick: number; slots: CommitSlot[] }
  | { type: "hashOk"; tick: number }
  | { type: "desync"; tick: number; hashes: { player: number; checksum: number }[] }
  | { type: "ended"; outcome: WireOutcome; replayId: string }
  | { type: "load"; save: unknown; you: ClientIdentity }
  | { type: "error"; code: string; message: string };

/** Chat is presentation traffic: control characters become spaces, at most 300 characters. */
export function chatText(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const text = value.replace(/[\u0000-\u001f\u007f]/g, " ").trim().slice(0, 300);
  return text || null;
}

/** Save format the server accepts (old `SAVE_FORMAT_VERSION`). */
const SAVE_FORMAT_VERSION = 3;

export type PipelineSnap = {
  committed: number;
  through: { player: number; through: number }[];
  held: { player: number; tick: number; actions: Action[] }[];
  commits: Commit[];
  sentThrough: number;
};

const natural = z.number().int().nonnegative();
const pipelineSchema = z
  .object({
    committed: natural,
    sentThrough: natural,
    through: z.array(z.object({ player: natural, through: natural }).strict()),
    held: z.array(z.object({ player: natural, tick: natural, actions: z.array(actionSchema) }).strict()),
    commits: z.array(
      z.object({ tick: natural, slots: z.array(z.object({ player: natural, actions: z.array(actionSchema) }).strict()) }).strict(),
    ),
  })
  .strict();

function parseMatchConfig(raw: unknown): MatchConfig | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (o.v !== 1) return null;
  if (typeof o.roomId !== "string" || typeof o.mapId !== "string" || typeof o.mapRevision !== "string") return null;
  if (typeof o.seed !== "number" || typeof o.delay !== "number" || typeof o.checksumEvery !== "number") return null;
  if (typeof o.tickMs !== "number" || !Array.isArray(o.slots) || o.slots.length < 1) return null;
  // The game validates a company's members; the server only carries it along.
  const company = o.company;
  if (company !== undefined && (!Array.isArray(company) || company.length < 1 || company.length > 32)) return null;
  const slots: Slot[] = [];
  for (const s of o.slots) {
    if (!s || typeof s !== "object") return null;
    const r = s as Record<string, unknown>;
    if (typeof r.player !== "number" || (r.kind !== "human" && r.kind !== "ai")) return null;
    if (r.race !== undefined && (typeof r.race !== "string" || !/^[a-z][a-z0-9-]*$/.test(r.race))) return null;
    if (r.hero !== undefined && !idSchema.safeParse(r.hero).success) return null;
    slots.push({
      player: r.player,
      kind: r.kind,
      ...(typeof r.race === "string" ? { race: r.race } : {}),
      ...(typeof r.hero === "string" ? { hero: r.hero } : {}),
      ...(typeof r.team === "number" ? { team: r.team } : {}),
      name: typeof r.name === "string" ? r.name : undefined,
    });
  }
  return {
    v: 1,
    roomId: o.roomId,
    mapId: o.mapId,
    mapRevision: o.mapRevision,
    seed: o.seed,
    delay: o.delay,
    checksumEvery: o.checksumEvery,
    tickMs: o.tickMs as MatchConfig["tickMs"],
    slots,
    ...(company !== undefined ? { company: company as unknown[] } : {}),
  };
}

/** What the host needs from a save to resume the mailbox; the world stays opaque. */
export function parseSaveForHost(raw: unknown): { match: MatchConfig; pipeline: PipelineSnap; remote: boolean } | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (o.v !== SAVE_FORMAT_VERSION) return null;
  if (typeof o.remote !== "boolean") return null;
  const match = parseMatchConfig(o.match);
  const pipeline = pipelineSchema.safeParse(o.pipeline);
  if (!match || !pipeline.success) return null;
  return { match, pipeline: pipeline.data, remote: o.remote };
}

/** The save's slots with the names of the players now in the room. */
export function namedMatch(match: MatchConfig, names: ReadonlyMap<number, string>): MatchConfig {
  return { ...match, slots: match.slots.map((s) => ({ ...s, name: names.get(s.player) ?? s.name })) };
}
