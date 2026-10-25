/**
 * The lobby and lockstep protocol (D10, reopened once by D107 as protocol 2,
 * docs/server.md#protocol-2): wire messages, the match config, the action shapes, chat
 * clean-up and the parts of a multiplayer save the host reads. The game client in
 * `crates/net` mirrors it.
 */
import { z } from "zod";

export const COMMAND_DELAY = 8;
export const CHECKSUM_EVERY = 8;
export const TICK_MS = 25;

/** The protocol this server speaks; create and join bodies must name it. */
export const PROTOCOL = 2;
/** A player's chosen identifier (D107): letters, digits, `.`, `_`, `-`, up to 32. */
export const validPlayerId = (x: unknown): x is string => typeof x === "string" && /^[A-Za-z0-9._-]{1,32}$/.test(x);

/** Whether a create or join body may enter: the right protocol and a well-formed player id. */
export function admit(body: unknown): { status: number; body: { error: string; message?: string } } | null {
  const o = body && typeof body === "object" ? (body as Record<string, unknown>) : {};
  if (o.protocol !== PROTOCOL)
    return { status: 426, body: { error: "update_required", message: "Update Under the Canopy to play online." } };
  if (!validPlayerId(o.playerId)) return { status: 400, body: { error: "bad_player_id" } };
  return null;
}

/** A computer seat's difficulty; Normal travels as no difficulty at all. */
export type Difficulty = "easy" | "hard";
export const DIFFICULTIES = ["easy", "normal", "hard"] as const;

/** Spectator places a lobby may offer, and the observer delays it may set (seconds). */
export const MAX_SPECTATOR_PLACES = 4;
export const DEFAULT_SPECTATOR_PLACES = 2;
export const OBSERVER_DELAYS = [0, 60, 120, 180] as const;
/** Beats of `seconds` of match time. */
export const beatsOf = (seconds: number) => (seconds * 1000) / TICK_MS;

export type Slot = {
  player: number;
  kind: "human" | "ai";
  name?: string;
  team?: number;
  hero?: string;
  race?: string;
  /** Computer seats only; absent for Normal. */
  difficulty?: Difficulty;
};

/** Teams and palette colours a lobby offers. */
export const TEAMS = 8;
export const COLORS = 8;

/** What a lobby seat holds: open for a human to join, locked by the host, or a computer. */
export type Seat = "open" | "closed" | "ai";

export type MatchConfig = {
  v: 2;
  roomId: string;
  mapId: string;
  mapRevision: string;
  seed: number;
  delay: number;
  checksumEvery: number;
  tickMs: typeof TICK_MS;
  slots: Slot[];
  /** How far spectators stay behind the players, in beats. */
  observerDelay: number;
  /** Palette colour of each slot, in slot order (presentation only). Kept out of `Slot`,
   * which game versions up to 0.3.1 read strictly. */
  colors?: number[];
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

/** Every action a commit may hold: the client-writable gameplay intentions, and `away` and
 * `back`, which only the server writes (a seat dropped from and taken back into the lockstep). */
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
  // A map ping for the sender's team; without a position (older games) it marks nothing.
  z.object({ type: z.literal("ping"), position: pointSchema.optional() }).strict(),
  z.object({ type: z.literal("away") }).strict(),
  z.object({ type: z.literal("back") }).strict(),
]);
export type Action = z.infer<typeof actionSchema>;
export const validAction = (value: unknown): value is Action => actionSchema.safeParse(value).success;
/** Actions only the server writes into commits. */
const SERVER_ACTIONS = new Set(["away", "back"]);
/** An action a client may send: any valid one except the server's own. */
export const validClientAction = (value: unknown): value is Action =>
  validAction(value) && !SERVER_ACTIONS.has(value.type);

export type WireOutcome = { winner: number | null; defeated: number[] };
export type Bundle = { tick: number; actions: Action[] };
export type CommitSlot = { player: number; actions: Action[] };
export type Commit = { tick: number; slots: CommitSlot[] };
export type ClientIdentity = { role: "player" | "spectator"; player?: number; name: string };
export type RoomState = "waiting" | "playing" | "ended" | "desynced";

export type Observer = { id: string; name: string; playerId: string };

export type RoomView = {
  id: string;
  state: RoomState;
  name: string;
  mapId: string;
  host: string;
  slots: {
    player: number;
    name: string | null;
    roundTripMs?: number;
    hero?: string;
    race?: string;
    /** `human` once someone joined; absent from servers before seats existed. */
    kind?: Seat | "human";
    team?: number;
    color?: number;
    /** Human seats: the player's chosen id. */
    playerId?: string;
    /** Computer seats: `easy` or `hard`; absent for Normal. */
    difficulty?: Difficulty;
    /** During a match: the seat's player was dropped or left. */
    away?: true;
  }[];
  /** How many spectators there are (older games read only this). */
  spectators: number;
  spectatorPlaces: number;
  observers: Observer[];
  /** Seconds. */
  observerDelay: number;
  tick?: number;
  inputDelayMs?: number;
};

export type CreateRoom = { name: string; mapId: string; mapRevision: string; slotCount: number; guestName: string; playerId: string; protocol?: number };
export type JoinRoom = { guestName: string; role: "player" | "spectator"; playerId: string; protocol?: number };
export type ChatMessage = { name: string; player: number | null; text: string };

export type ClientMsg =
  | { type: "latencyReply"; id: string }
  | { type: "chat"; text: string }
  | { type: "selectHero"; hero: string; player?: number }
  | { type: "selectRace"; race: string; player?: number }
  | { type: "setSeat"; player: number; seat: Seat; difficulty?: (typeof DIFFICULTIES)[number] }
  | { type: "setSpectatorPlaces"; count: number }
  | { type: "setObserverDelay"; seconds: number }
  | { type: "kick"; player?: number; observer?: string }
  | { type: "takeSeat"; player: number }
  | { type: "spectate" }
  | { type: "dropPlayer"; player: number }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "selectTeam"; team: number; player?: number }
  | { type: "selectColor"; color: number; player?: number }
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
  | { type: "history"; through: number; commits: Commit[] }
  | { type: "waiting"; players: number[]; seconds: number }
  | { type: "dropped" }
  | { type: "kicked" }
  | { type: "paused"; player: number; name: string; pausesLeft: number }
  | { type: "resuming"; seconds: number }
  | { type: "resumed" }
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
  // Saves from protocol 1 carry `v: 1`; their config reads the same, without a delay.
  if (o.v !== 1 && o.v !== 2) return null;
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
      ...(r.kind === "ai" && (r.difficulty === "easy" || r.difficulty === "hard") ? { difficulty: r.difficulty } : {}),
      name: typeof r.name === "string" ? r.name : undefined,
    });
  }
  return {
    v: 2,
    roomId: o.roomId,
    mapId: o.mapId,
    mapRevision: o.mapRevision,
    seed: o.seed,
    delay: o.delay,
    checksumEvery: o.checksumEvery,
    tickMs: o.tickMs as MatchConfig["tickMs"],
    slots,
    observerDelay: typeof o.observerDelay === "number" && Number.isInteger(o.observerDelay) && o.observerDelay >= 0 ? o.observerDelay : 0,
    ...(Array.isArray(o.colors) && o.colors.length === slots.length && o.colors.every((c) => Number.isInteger(c) && c >= 0 && c < COLORS)
      ? { colors: o.colors as number[] }
      : {}),
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
