/**
 * One MatchHost room: lobby until Start, then the lockstep Room.
 * HTTP/WS bind `ingest` / `bind`. `discard` kills a room (tests + `/end`). Tests call the same methods with fake sends.
 * Protocol 2 (docs/server.md#protocol-2): spectator places and the observer delay, moving
 * between seats, kicks, rejoining from the commit log, waiting, dropping and pausing.
 */
import {ConnectionLatency,connectionDelay} from './latency';
import {
  CHECKSUM_EVERY,
  COLORS,
  DEFAULT_SPECTATOR_PLACES,
  DIFFICULTIES,
  MAX_SPECTATOR_PLACES,
  OBSERVER_DELAYS,
  TEAMS,
  TICK_MS,
  beatsOf,
  type Difficulty,
  type Seat,
  chatText,
  namedMatch,
  parseSaveForHost,
  validClientAction,
  type ClientIdentity,
  type ClientMsg,
  type CreateRoom,
  type MatchConfig,
  type PipelineSnap,
  type QueueMode,
  type RoomState,
  type RoomView,
  type ServerMsg,
  type Slot,
  type WireOutcome,
} from "./protocol";
import { Room } from "./room";

/** A content id (`unit.ants.marshal`, `ants`). */
const validId=(x:unknown):x is string=>typeof x==='string'&&x.length<=160&&/^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/.test(x);

/** Pauses each player has in a match. */
export const PAUSES_PER_MATCH = 3;
/** How long the beats wait for a seat before everyone is told (ms). */
export const WAITING_NOTICE_AFTER_MS = 5_000;
/** How long the beats wait for a seat before the others may drop it (ms). */
export const DROP_AFTER_MS = 30_000;
/** How long after a pause anyone but the pauser may resume (ms). */
export const RESUME_BY_OTHERS_AFTER_MS = 30_000;
/** The countdown between `resuming` and `resumed` (s). */
export const RESUME_COUNTDOWN_S = 3;
/** How long a queue's room waits for both players' sockets before it is cancelled (ms). */
export const QUEUE_BIND_MS = 30_000;
/** A queue's room: spectator places, and the observer delay of each queue (s). */
export const QUEUE_SPECTATOR_PLACES = 2;
export const QUEUE_OBSERVER_DELAY: Record<QueueMode, number> = { ranked: 120, unranked: 0 };
/** Lobby messages, which a queue's room refuses: it has no lobby. */
const LOBBY_MESSAGES = new Set<ClientMsg["type"]>([
  "startMatch", "selectRace", "selectHero", "selectTeam", "selectColor", "setSeat", "setSpectatorPlaces",
  "setObserverDelay", "kick", "takeSeat", "spectate", "loadSave", "restart",
]);

/** A player a queue seats in a room, with the race already resolved. */
export type QueuedSeat = { name: string; playerId: string; race: string };
/** What a game reported at the end of a match: `ended` from a seated player. */
export type EndedReport = { player: number; outcome: unknown; tick: unknown };
/** A room a queue fills: seat 0 and seat 1, and who hears the players' `ended` reports. */
export type QueuedMatch = {
  mode: QueueMode;
  mapId: string;
  seats: readonly [QueuedSeat, QueuedSeat];
  onReport?: (report: EndedReport) => void;
};

/** Runs `fn` after `ms` milliseconds; the result cancels it. */
export type Schedule = (fn: () => void, ms: number) => () => void;
const realSchedule: Schedule = (fn, ms) => {
  const timer = setTimeout(fn, ms);
  timer.unref?.();
  return () => clearTimeout(timer);
};

export type HostOptions = {
  /** Timers (the resume countdown); tests pass their own. */
  schedule?: Schedule;
  /** The server's log lines: creates, joins, leaves, drops, results. */
  log?: (line: string) => void;
};

type Member = {
  token: string;
  /** Short public id: how the room view names a spectator; never the token. */
  id: string;
  name: string;
  playerId: string;
  role: "player" | "spectator";
  player?: number;
  hero?: string;
  race?: string;
  send: ((msg: ServerMsg) => void) | null;
  /** Closes the member's socket, if the transport can. */
  close: (() => void) | null;
  latency: ConnectionLatency;
  /** Spectators: the last tick whose commit they have been sent. */
  sentThrough: number;
};

/** A lobby seat: what holds it, its team and colour, and a computer's race, hero and difficulty. */
type SeatState = {
  seat: Seat;
  team: number;
  color: number;
  race?: string;
  hero?: string;
  difficulty?: Difficulty;
};

export class HostedMatch {
  readonly id: string;
  readonly name: string;
  readonly mapId: string;
  readonly mapRevision: string;
  readonly slotCount: number;
  readonly hostToken: string;
  private state: RoomState = "waiting";
  private readonly members = new Map<string, Member>();
  private readonly ready = new Set<number>();
  private contentAgreement:string|null=null;
  private started=false;
  /** When the last player connection closed (`now()`), or null while one is open. */
  private emptySince:number|null;
  private readonly hashes = new Map<number, Map<number, number>>();
  /** The hashes every player agreed on at a tick, for hashes that arrive later (a rejoin). */
  private readonly agreed = new Map<number, Map<number, number>>();
  private mailbox: Room | null = null;
  private config: MatchConfig | null = null;
  private replayId: string | null = null;
  private lastSave: unknown = null;
  private chatTimes = new Map<string, number>();
  private readonly seats: SeatState[];
  private spectatorPlaces = DEFAULT_SPECTATOR_PLACES;
  /** Seconds. */
  private observerDelay = 0;
  /** Player ids of the match's human seats, kept for seats whose player left. */
  private seatIds = new Map<number, string>();
  private pausesLeft = new Map<number, number>();
  /** The pause under way (until `resumed`): who paused and when. */
  private pause: { player: number; at: number } | null = null;
  private cancelResume: (() => void) | null = null;
  /** When the beats last moved: the last commit, `go`, or that moment shifted by a pause. */
  private lastProgress = 0;
  private waitingShown = false;
  private readonly schedule: Schedule;
  private readonly logLine: (line: string) => void;
  /** The queue that filled this room (no lobby), or null for a lobby's room. */
  private queue: QueueMode | null = null;
  /** When the queue made the room: both sockets must be bound `QUEUE_BIND_MS` after. */
  private queuedAt = 0;
  private onReport: ((report: EndedReport) => void) | null = null;

  constructor(draft: CreateRoom, id: string = crypto.randomUUID(), private readonly now=()=>performance.now(), options: HostOptions = {}) {
    this.schedule = options.schedule ?? realSchedule;
    this.logLine = options.log ?? ((line) => console.log(line));
    this.id = id;
    this.name = draft.name;
    this.mapId = draft.mapId;
    this.mapRevision = draft.mapRevision;
    this.slotCount = Math.min(8, Math.max(2, draft.slotCount | 0));
    // Free for all by default: every seat its own team and its own colour.
    this.seats = Array.from({ length: this.slotCount }, (_, i) => ({ seat: "open" as Seat, team: i + 1, color: i % COLORS }));
    this.hostToken = token();
    // The host creates the room over HTTP before opening its socket.
    this.emptySince = now();
    const host = this.member(this.hostToken, draft.guestName, draft.playerId, "player", 0);
    this.members.set(this.hostToken, host);
    this.log(`created by ${who(host)}, map ${this.mapId}`);
  }

  private member(t: string, name: string, playerId: string, role: Member["role"], player?: number): Member {
    let id = shortId();
    while ([...this.members.values()].some((m) => m.id === id)) id = shortId();
    return { token: t, id, name, playerId, role, ...(player != null ? { player } : {}), send: null, close: null, latency: new ConnectionLatency(), sentThrough: 0 };
  }

  private log(line: string) {
    this.logLine(`room ${this.id}: ${line}`);
  }

  /** Makes this fresh room a queue's: the creator in seat 0, the other player in seat 1,
   * their races set, teams 1 and 2, colours 0 and 1, no lobby. Returns both seats' tokens. */
  queued(setup: QueuedMatch): [string, string] {
    this.queue = setup.mode;
    this.onReport = setup.onReport ?? null;
    this.queuedAt = this.now();
    this.spectatorPlaces = QUEUE_SPECTATOR_PLACES;
    this.observerDelay = QUEUE_OBSERVER_DELAY[setup.mode];
    const first = this.members.get(this.hostToken)!;
    first.race = setup.seats[0].race;
    const t = token();
    const second = this.member(t, setup.seats[1].name, setup.seats[1].playerId, "player", 1);
    second.race = setup.seats[1].race;
    this.members.set(t, second);
    this.seats.forEach((seat, i) => Object.assign(seat, { seat: "open", team: i + 1, color: i }));
    this.log(`${setup.mode} queue: seat 0 ${who(first)} ${first.race}, seat 1 ${who(second)} ${second.race}`);
    return [this.hostToken, t];
  }

  /** Where the room is: lobby, match, ended or desynced. */
  get status(): RoomState {
    return this.state;
  }

  /** The queue that filled this room, if one did. */
  get queueMode(): QueueMode | null {
    return this.queue;
  }

  /** Whether seat `player` of the match is away: dropped or left. */
  isAway(player: number): boolean {
    return !!this.mailbox?.isDropped(player);
  }

  /** A queue's room ends before its match: every bound player hears `MATCH_CANCELLED`. */
  private cancel(why: string) {
    this.log(`cancelled: ${why}`);
    this.state = "ended";
    for (const m of this.members.values()) {
      if (!m.send) continue;
      m.send({ type: "error", code: "MATCH_CANCELLED", message: "The other player did not arrive. Search again." });
      this.disconnect(m);
    }
  }

  // Races and heroes are content: the server passes the seats' choices on, unchecked
  // (clients agree on content before a match starts); a seat without one plays the
  // content's default race and its default hero.
  private choices(x:{race?:string;hero?:string}){return {...(x.race?{race:x.race}:{}),...(x.hero?{hero:x.hero}:{})};}
  /** The human in seat `i`, if any. */
  private seated(i:number){return [...this.members.values()].find((x) => x.role === "player" && x.player === i);}
  private spectators(){return [...this.members.values()].filter((m) => m.role === "spectator");}
  /** Whether seat `i` takes part: a human sits there or a computer plays it. */
  private occupied(i:number){return !!this.seated(i)||this.seats[i]?.seat==='ai';}
  private colorFree(color:number,except:number){return !this.seats.some((s,j)=>j!==except&&this.occupied(j)&&s.color===color);}
  /** Seat `i` keeps its colour when nobody taking part has it, else gets the first free one. */
  private claimColor(i:number){
    const s=this.seats[i]!;
    if(this.colorFree(s.color,i))return;
    for(let c=0;c<COLORS;c++)if(this.colorFree(c,i)){s.color=c;return;}
  }
  /** The seat `msg.player` names (else the sender's own) if the sender may change it: their
   * own seat, or a computer's when they host. */
  private editable(m:Member,player:unknown):number|null{
    if(this.state!=='waiting')return null;
    const target=player===undefined?m.player:player;
    if(typeof target!=='number'||!Number.isInteger(target)||target<0||target>=this.slotCount)return null;
    if(m.role==='player'&&target===m.player)return target;
    return m.token===this.hostToken&&!this.seated(target)&&this.seats[target]!.seat==='ai'?target:null;
  }
  /** Whether `player` is a human seat of the match under way. */
  private humanSlot(player:number|undefined){return player!=null&&!!this.config?.slots.some((s)=>s.player===player&&s.kind==='human');}
  view(): RoomView {
    const seats: RoomView['slots'] = [];
    const live = this.state !== "waiting" && this.config ? this.config : null;
    for (let i = 0; i < this.slotCount; i++) {
      const m = this.seated(i);
      const s = this.seats[i]!;
      const rtt=m?.latency.roundTrip(this.now());
      const shown = { team: s.team, color: s.color };
      const away = live && this.mailbox?.isDropped(i) ? { away: true as const } : {};
      const left = live?.slots.find((slot) => slot.player === i && slot.kind === "human");
      if (m) seats.push({ player: i, name: m.name, kind: "human", playerId: m.playerId, ...this.choices(m), ...shown, ...away, ...(rtt!=null?{roundTripMs:rtt}:{}) });
      else if (left) {
        const playerId = this.seatIds.get(i);
        seats.push({ player: i, name: left.name ?? null, kind: "human", ...(playerId ? { playerId } : {}), ...this.choices(left), ...shown, ...away });
      }
      else if (s.seat === "ai") seats.push({ player: i, name: "Computer", kind: "ai", ...this.choices(s), ...(s.difficulty ? { difficulty: s.difficulty } : {}), ...shown });
      else seats.push({ player: i, name: null, kind: s.seat, ...shown });
    }
    const observers = this.spectators().map((m) => ({ id: m.id, name: m.name, playerId: m.playerId }));
    return {
      id: this.id,
      state: this.state,
      name: this.name,
      mapId: this.mapId,
      host: this.members.get(this.hostToken)?.name ?? "",
      slots: seats,
      spectators: observers.length,
      spectatorPlaces: this.spectatorPlaces,
      observers,
      observerDelay: this.observerDelay,
      tick: this.mailbox?.tick,
      inputDelayMs: (this.config?.delay ?? this.chooseDelay())*TICK_MS,
      ...(this.queue ? { queue: this.queue } : {}),
    };
  }

  you(token: string): ClientIdentity | null {
    const m = this.members.get(token);
    if (!m) return null;
    return m.role === "player"
      ? { role: "player", player: m.player, name: m.name }
      : { role: "spectator", name: m.name };
  }

  join(
    guestName: string,
    role: "player" | "spectator",
    playerId: string,
  ): { token: string; you: ClientIdentity } | { error: string } {
    if (
      role === "spectator" &&
      (this.state === "waiting" || this.state === "playing")
    ) {
      // A lobby has its spectator places; a match under way takes any spectator.
      if (this.state === "waiting" && this.spectators().length >= this.spectatorPlaces) return { error: "full" };
      const t = token();
      const m = this.member(t, guestName, playerId, "spectator");
      m.sentThrough = this.mailbox?.tick ?? 0;
      this.members.set(t, m);
      this.log(`${who(m)} joined as a spectator`);
      this.fanout({ type: "room", room: this.view() });
      return { token: t, you: { role: "spectator", name: guestName } };
    }
    if (this.state !== "waiting") return { error: "not_waiting" };
    // A queue's two seats are its matched players'.
    if (this.queue) return { error: "full" };
    let player = 0;
    while (player < this.slotCount && (this.seated(player) || this.seats[player]!.seat !== "open")) player++;
    if (player >= this.slotCount) return { error: "full" };
    const t = token();
    const m = this.member(t, guestName, playerId, "player", player);
    this.members.set(t, m);
    this.claimColor(player);
    this.log(`${who(m)} joined seat ${player}`);
    this.fanout({ type: "room", room: this.view() });
    return { token: t, you: { role: "player", player, name: guestName } };
  }

  leave(auth: string): void {
    const m = this.members.get(auth);
    if (!m) return;
    this.log(`${who(m)} left`);
    if (this.state === "waiting" && this.queue) {
      // A queue's match cannot start without both players.
      this.members.delete(auth);
      if (m.role === "player") this.cancel(`${who(m)} left`);
      else this.fanout({ type: "room", room: this.view() });
      return;
    }
    if (this.state === "waiting") {
      if (auth === this.hostToken) {
        this.state = "ended";
        this.members.clear();
        return;
      }
      this.members.delete(auth);
      this.fanout({ type: "room", room: this.view() });
      return;
    }
    // A deliberate leave during a match drops the seat at once; the token is void.
    this.members.delete(auth);
    if (this.state === "playing" && m.role === "player" && m.player != null && this.dropSeat(m.player, "left", m)) this.tryGo();
    else this.disconnect(m);
  }

  /** Takes a seat out of the lockstep; the seat's open socket gets `dropped` and closes. */
  private dropSeat(player: number, why: string, m: Member | undefined = this.seated(player)): boolean {
    if (!this.mailbox?.drop(player)) return false;
    this.log(`seat ${player} ${m ? who(m) : ""} dropped (${why}) at tick ${this.mailbox.tick}`);
    if (m?.send) {
      m.send({ type: "dropped" });
      this.disconnect(m);
    }
    this.fanout({ type: "room", room: this.view() });
    return true;
  }

  /** Forgets a member's socket and closes it. */
  private disconnect(m: Member) {
    const close = m.close;
    m.send = null;
    m.close = null;
    m.latency.reset();
    close?.();
  }

  start(auth: string): { error: string } | { config: MatchConfig } {
    // A queue's room starts by itself once both players are bound.
    if (this.queue) return { error: "queue_room" };
    if (auth !== this.hostToken) return { error: "not_host" };
    return this.launch();
  }

  private launch(): { error: string } | { config: MatchConfig } {
    if (this.state !== "waiting") return { error: "not_waiting" };
    const players = [...this.members.values()]
      .filter((m) => m.role === "player" && m.player != null)
      .sort((a, b) => (a.player ?? 0) - (b.player ?? 0));
    if (players.length < 1) return { error: "empty" };
    // Seats nobody joined, and closed ones, stay out of the match: no base is built there.
    const slots: Slot[] = [], colors: number[] = [];
    for (let i = 0; i < this.slotCount; i++) {
      const m = this.seated(i), s = this.seats[i]!;
      if (m) slots.push({ player: i, kind: "human", name: m.name, ...this.choices(m), team: s.team });
      else if (s.seat === "ai") slots.push({ player: i, kind: "ai", name: "Computer", ...this.choices(s), team: s.team, ...(s.difficulty ? { difficulty: s.difficulty } : {}) });
      else continue;
      colors.push(s.color);
    }
    const config: MatchConfig = {
      v: 2,
      roomId: this.id,
      mapId: this.mapId,
      mapRevision: this.mapRevision,
      seed: seedU32(),
      delay: this.chooseDelay(),
      checksumEvery: CHECKSUM_EVERY,
      tickMs: TICK_MS,
      slots,
      observerDelay: beatsOf(this.observerDelay),
      colors,
      ...(this.queue ? { queue: this.queue } : {}),
    };
    this.seatIds = new Map(players.map((m) => [m.player!, m.playerId]));
    this.begin(config);
    this.state = "playing";
    this.log(`started: ${players.map((m) => `seat ${m.player} ${who(m)}`).join(", ")}`);
    for (const m of this.members.values()) {
      const you = this.you(m.token);
      if (you) m.send?.({ type: "start", config, you });
    }
    return { config };
  }

  /** A fresh mailbox for `config` (resumed from `snap` after a load): no pause, every
   * player's pauses back, seats whose player left dropped from the first beat. */
  private begin(config: MatchConfig, snap?: Pick<PipelineSnap, "committed" | "through" | "held">) {
    this.cancelResume?.();
    this.cancelResume = null;
    this.pause = null;
    this.waitingShown = false;
    this.hashes.clear();
    this.agreed.clear();
    this.ready.clear();this.contentAgreement=null;this.started=false;
    this.config = config;
    this.mailbox = new Room(config,false);
    if (snap) this.mailbox.resume(snap);
    this.mailbox.subscribe((msg) => this.onCommit(msg));
    const humans = config.slots.filter((s) => s.kind === "human").map((s) => s.player);
    this.pausesLeft = new Map(humans.map((p) => [p, PAUSES_PER_MATCH]));
    for (const m of this.members.values()) {
      if (m.role === "player" && m.player != null) this.seatIds.set(m.player, m.playerId);
      m.sentThrough = this.mailbox.tick;
    }
    for (const p of humans) if (!this.seated(p)) this.mailbox.drop(p);
  }

  /** Observer delay of the match under way, in beats. */
  private delayBeats(){return this.config?.observerDelay ?? 0;}

  /** A beat committed: players have it at once, spectators `observerDelay` beats later. */
  private onCommit(msg: ServerMsg) {
    if (msg.type !== "commit") return;
    this.lastProgress = this.now();
    for (const m of this.members.values()) if (m.role === "player") m.send?.(msg);
    this.releaseSpectators(msg.tick - this.delayBeats());
    if (this.waitingShown) {
      this.waitingShown = false;
      this.fanout({ type: "waiting", players: [], seconds: 0 });
    }
  }

  /** Sends every spectator the commits it has not had, up to `upTo`. */
  private releaseSpectators(upTo: number) {
    const mailbox = this.mailbox;
    if (!mailbox) return;
    upTo = Math.min(upTo, mailbox.tick);
    for (const m of this.spectators()) {
      if (!m.send) continue;
      while (m.sentThrough < upTo) {
        m.sentThrough++;
        m.send({ type: "commit", ...mailbox.commitAt(m.sentThrough) });
      }
    }
  }

  /**
   * Host loads a **multiplayer** save (`remote: true`). Lobby: `start+save`; live: `load`.
   * Mailbox resumes at the saved committed tick — clients restore the snapshot.
   */
  load(
    auth: string,
    save: unknown,
  ): { error: string } | { config: MatchConfig } {
    if (this.queue) return { error: "queue_room" };
    if (auth !== this.hostToken) return { error: "not_host" };
    if (this.state === "ended") return { error: "ended" };
    const parsed = parseSaveForHost(save);
    if (!parsed) return { error: "bad_save" };
    if (parsed.match.slots.length < 1) return { error: "empty" };
    if (!parsed.remote) return { error: "sp_save" };
    const players = [...this.members.values()].filter(
      (m) => m.role === "player" && m.player != null,
    );
    if (
      this.state === "waiting" &&
      players.length !== parsed.match.slots.filter((s) => s.kind === "human").length
    )
      return { error: "slots" };
    const live = this.state === "playing";
    const names = new Map<number, string>();
    for (const m of this.members.values()) {
      if (m.role === "player" && m.player != null) names.set(m.player, m.name);
    }
    // The room's observer delay holds, not the one the save was made with.
    const config = namedMatch({ ...parsed.match, roomId: this.id, observerDelay: beatsOf(this.observerDelay) }, names);
    for(const m of this.members.values()){const slot=config.slots.find(s=>s.player===m.player);if(slot){m.race=slot.race;m.hero=slot.hero;}}
    this.lastSave = save;
    this.begin(config, parsed.pipeline);
    this.state = "playing";
    this.log(`loaded a save at tick ${parsed.pipeline.committed}`);
    for (const m of this.members.values()) {
      const you = this.you(m.token);
      if (!you) continue;
      if (live) m.send?.({ type: "load", save, you });
      else m.send?.({ type: "start", config, you, save });
    }
    return { config };
  }

  /**
   * Same map/slots, new seed, empty mailbox. Clients rebuild kits from the dump.
   * Host F10 Restart. Does not reload a save.
   */
  restart(auth: string): { error: string } | { config: MatchConfig } {
    if (this.queue) return { error: "queue_room" };
    if (auth !== this.hostToken) return { error: "not_host" };
    if (this.state !== "playing" || !this.config)
      return { error: "not_playing" };
    const config: MatchConfig = { ...this.config, seed: seedU32(), delay:this.chooseDelay() };
    this.lastSave = null;
    this.begin(config);
    this.log("restarted");
    for (const m of this.members.values()) {
      const you = this.you(m.token);
      if (you) m.send?.({ type: "restart", config, you });
    }
    return { config };
  }

  /**
   * A member's socket opens. During a match it receives `start` and then `history`: the
   * commits after `since` that hold an action (a spectator's stay `observerDelay` beats behind).
   */
  bind(
    auth: string,
    send: (msg: ServerMsg) => void,
    options: { since?: number; close?: () => void } = {},
  ): { error: string } | { you: ClientIdentity; room: RoomView } {
    const m = this.members.get(auth);
    if (!m) return { error: "bad_token" };
    if (this.queue && this.state === "ended" && !this.config) {
      // Bound after the queue's room was cancelled, before it was discarded.
      send({ type: "error", code: "MATCH_CANCELLED", message: "The other player did not arrive. Search again." });
      options.close?.();
      return { error: "cancelled" };
    }
    m.send = send;
    m.close = options.close ?? null;
    m.latency.reset();
    const you = this.you(auth)!;
    send({ type: "welcome", you, room: this.view() });
    this.probeMember(m);
    if (this.state === "playing" && this.config && this.mailbox) {
      send({
        type: "start",
        config: this.config,
        you,
        save: this.lastSave ?? undefined,
      });
      const mailbox = this.mailbox;
      const since = Number.isSafeInteger(options.since) && options.since! > 0 ? options.since! : 0;
      const through = m.role === "spectator" ? Math.max(mailbox.logBase, mailbox.tick - this.delayBeats()) : mailbox.tick;
      send({ type: "history", through, commits: mailbox.history(since, through) });
      if (m.role === "spectator") m.sentThrough = Math.max(through, Math.min(since, mailbox.tick));
      // A reconnect must revalidate its content before receiving go.
      if(m.player!=null)this.ready.delete(m.player);
    }
    // A queue's room starts once both players' sockets are bound.
    if (this.queue && this.state === "waiting" && [0, 1].every((p) => this.seated(p)?.send)) this.launch();
    return { you, room: this.view() };
  }

  /** A member's socket closed. `send` names which one, so a socket replaced by a newer one
   * does not disconnect it. The beats keep waiting for a player who is gone. */
  unbind(auth: string, send?: (msg: ServerMsg) => void): void {
    const m = this.members.get(auth);
    if (!m) return;
    if (send && m.send !== send) return;
    m.send = null;
    m.close = null;
    m.latency.reset();
    if (this.state === "playing" && m.role === "player" && m.player != null) this.tryGo();
  }

  ingest(auth: string, msg: ClientMsg): void {
    const m = this.members.get(auth);
    if (!m) return;
    const refuse = (code: string, message: string) => m.send?.({ type: "error", code, message });
    const host = m.token === this.hostToken;
    const waiting = this.state === "waiting";
    if (this.queue && LOBBY_MESSAGES.has(msg.type)) {
      refuse("LOBBY_REFUSED", "A queue's match has no lobby.");
      return;
    }
    if(msg.type==='startMatch') {
      const result=this.start(auth);
      if('error' in result)refuse('START_REJECTED',result.error);
      return;
    }
    if(msg.type==='selectRace') {
      const seat=this.editable(m,msg.player);
      if(seat===null)return;
      if(!validId(msg.race)){refuse('INVALID_RACE','Choose an available race.');return;}
      if(seat===m.player&&m.role==='player'){m.race=msg.race;m.hero=undefined;}
      else{const s=this.seats[seat]!;s.race=msg.race;s.hero=undefined;}
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='selectHero') {
      const seat=this.editable(m,msg.player);
      if(seat===null)return;
      const own=seat===m.player&&m.role==='player',s=this.seats[seat]!;
      if(!validId(msg.hero)){
        refuse('INVALID_HERO','Choose an available starting hero.');return;
      }
      if(own)m.hero=msg.hero;else s.hero=msg.hero;
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='selectTeam') {
      const seat=this.editable(m,msg.player);
      if(seat===null)return;
      if(!Number.isInteger(msg.team)||msg.team<1||msg.team>TEAMS){refuse('INVALID_TEAM','Choose a team.');return;}
      this.seats[seat]!.team=msg.team;this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='selectColor') {
      const seat=this.editable(m,msg.player);
      if(seat===null)return;
      if(!Number.isInteger(msg.color)||msg.color<0||msg.color>=COLORS||!this.colorFree(msg.color,seat)){
        refuse('INVALID_COLOR','That colour is taken.');return;
      }
      this.seats[seat]!.color=msg.color;this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='setSeat') {
      // The host opens, closes or hands a seat to the computer; a joined human keeps theirs.
      if(!waiting||!host)return;
      const seat=msg.player;
      const difficulty=msg.difficulty;
      if(!Number.isInteger(seat)||seat<0||seat>=this.slotCount||this.seated(seat)||!['open','closed','ai'].includes(msg.seat)||(difficulty!==undefined&&!DIFFICULTIES.includes(difficulty))){
        refuse('INVALID_SEAT','That seat cannot change.');return;
      }
      const s=this.seats[seat]!;
      // A computer seat keeps its race and hero when only its difficulty changes.
      if(s.seat!==msg.seat){s.race=undefined;s.hero=undefined;s.difficulty=undefined;}
      s.seat=msg.seat;
      if(s.seat==='ai'){
        if(difficulty!==undefined)s.difficulty=difficulty==='normal'?undefined:difficulty;
        this.claimColor(seat);
      }
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='setSpectatorPlaces') {
      if(!waiting||!host)return;
      const count=msg.count;
      if(!Number.isInteger(count)||count<0||count>MAX_SPECTATOR_PLACES||count<this.spectators().length){
        refuse('INVALID_SPECTATOR_PLACES','There cannot be that many spectator places.');return;
      }
      this.spectatorPlaces=count;this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='setObserverDelay') {
      if(!waiting||!host)return;
      if(!(OBSERVER_DELAYS as readonly number[]).includes(msg.seconds)){
        refuse('INVALID_OBSERVER_DELAY','Choose an observer delay of 0, 1, 2 or 3 minutes.');return;
      }
      this.observerDelay=msg.seconds;this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='kick') {
      if(!waiting||!host)return;
      const target=msg.player!==undefined
        ?(Number.isInteger(msg.player)?this.seated(msg.player):undefined)
        :this.spectators().find((x)=>typeof msg.observer==='string'&&x.id===msg.observer);
      if(!target||target.token===this.hostToken){refuse('INVALID_KICK','Nobody to kick there.');return;}
      this.members.delete(target.token);
      this.log(`${who(target)} kicked`);
      target.send?.({type:'kicked'});
      this.disconnect(target);
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='takeSeat') {
      if(!waiting)return;
      const seat=msg.player;
      if(!Number.isInteger(seat)||seat<0||seat>=this.slotCount||this.seated(seat)||this.seats[seat]!.seat!=='open'){
        refuse('INVALID_SEAT','That seat is not open.');return;
      }
      m.role='player';m.player=seat;
      this.claimColor(seat);
      // Its identity changed: the mover hears it as on joining.
      m.send?.({type:'welcome',you:this.you(auth)!,room:this.view()});
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='spectate') {
      if(!waiting||m.role!=='player')return;
      if(this.spectators().length>=this.spectatorPlaces){refuse('SPECTATORS_FULL','No spectator place is free.');return;}
      m.role='spectator';m.player=undefined;m.race=undefined;m.hero=undefined;
      m.send?.({type:'welcome',you:this.you(auth)!,room:this.view()});
      this.fanout({type:'room',room:this.view()});return;
    }
    if(msg.type==='latencyReply'){
      if(m.send&&typeof msg.id==='string'&&m.latency.reply(msg.id,this.now())){
        this.probeMember(m);
        if(this.state==='waiting')this.fanout({type:'room',room:this.view()});
      }
      return;
    }
    if (msg.type === "chat") {
      if (!m.send || this.state !== "playing") return;
      const text = chatText(msg.text), now = this.now();
      if (!text || now - (this.chatTimes.get(auth) ?? -Infinity) < 500) return;
      this.chatTimes.set(auth, now);
      const line: ServerMsg = {type: "chat", message: {name: m.name, player: m.player ?? null, text}};
      // A spectator's line reaches spectators only; a player's reaches everyone.
      if (m.role === "spectator") for (const s of this.spectators()) s.send?.(line);
      else this.fanout(line);
      return;
    }
    if (msg.type === "hello") return;
    if (msg.type === "loadSave") {
      if (m.token !== this.hostToken) return;
      this.load(auth, msg.save);
      return;
    }
    if (msg.type === "restart") {
      if (m.token !== this.hostToken) return;
      this.restart(auth);
      return;
    }
    if (msg.type === "ready") {
      if (this.state !== "playing" || m.role !== "player" || m.player == null)
        return;
      const valid=msg.content&&typeof msg.content.abi==='string'&&typeof msg.build==='string'&&/^[a-f0-9]{64}$/.test(msg.content.sha256);
      const identity=valid?JSON.stringify([msg.build,msg.content.abi,msg.content.sha256]):null;
      if(!identity||(this.contentAgreement!==null&&identity!==this.contentAgreement)){
        this.state='desynced';this.fanout({type:'error',code:'CONTENT_MISMATCH',message:'Players have different engine or published ability content. Reload with matching content.'});return;
      }
      this.contentAgreement=identity;
      this.ready.add(m.player);
      if(this.started){
        // A late ready is a player who caught up: back into the lockstep from the last commit.
        if(this.mailbox?.rejoin(m.player)){
          this.log(`seat ${m.player} ${who(m)} is back at tick ${this.mailbox.tick + 1}`);
          this.fanout({type:'room',room:this.view()});
        }
        m.send?.({type:'go',tick:(this.mailbox?.tick??0)+1});
      }
      else this.tryGo();
      return;
    }
    if (msg.type === "turn") {
      if(this.state!=="playing")return;
      if (
        m.role !== "player" ||
        m.player == null ||
        !this.mailbox ||
        !this.config
      )
        return;
      // `away` and `back` are the server's: a turn holding them is refused whole.
      if (
        !Array.isArray(msg.bundles) ||
        msg.bundles.length > 64 ||
        !msg.bundles.every(
          (b) =>
            b &&
            Array.isArray(b.actions) &&
            b.actions.length <= 64 &&
            b.actions.every(validClientAction),
        )
      )
        return;
      const delay = this.config.delay;
      const bundles = msg.bundles
        .map((b) => ({
          tick: b.tick,
          actions: b.actions.filter((a) => a.type !== "noop"),
        }))
        .filter((b) => b.tick >= 1 && b.tick <= msg.through + delay + 2);
      this.mailbox.confirm(m.player, msg.through, bundles);
      return;
    }
    if (msg.type === "dropPlayer") {
      if (this.state !== "playing" || !this.mailbox || m.role !== "player" || !this.humanSlot(m.player) || this.mailbox.isDropped(m.player!)) return;
      const target = msg.player;
      if (target === m.player || !this.mailbox.waitingFor().includes(target) || this.waitedMs() < DROP_AFTER_MS) {
        refuse("DROP_REFUSED", "That player can be dropped once the match has waited 30 seconds for them.");
        return;
      }
      this.dropSeat(target, `dropped by ${who(m)}`);
      return;
    }
    if (msg.type === "pause") {
      if (this.state !== "playing" || !this.mailbox || m.role !== "player" || !this.humanSlot(m.player) || this.mailbox.isDropped(m.player!)) return;
      const left = this.pausesLeft.get(m.player!) ?? 0;
      if (!this.started || this.pause || left < 1) {
        refuse("PAUSE_REFUSED", left < 1 ? "No pauses left." : "The match cannot pause now.");
        return;
      }
      this.pausesLeft.set(m.player!, left - 1);
      this.pause = { player: m.player!, at: this.now() };
      this.mailbox.setPaused(true);
      this.log(`paused by seat ${m.player} ${who(m)}`);
      this.fanout({ type: "paused", player: m.player!, name: m.name, pausesLeft: left - 1 });
      return;
    }
    if (msg.type === "resume") {
      if (this.state !== "playing" || m.role !== "player" || !this.humanSlot(m.player)) return;
      const pause = this.pause;
      if (!pause || this.cancelResume) return;
      if (m.player !== pause.player && this.now() - pause.at < RESUME_BY_OTHERS_AFTER_MS) {
        refuse("RESUME_REFUSED", "Only the player who paused can resume yet.");
        return;
      }
      this.fanout({ type: "resuming", seconds: RESUME_COUNTDOWN_S });
      this.cancelResume = this.schedule(() => this.resumed(), RESUME_COUNTDOWN_S * 1000);
      return;
    }
    if (msg.type === "hash") {
      if (m.role !== "player" || m.player == null || this.state !== "playing")
        return;
      // A hash for a tick everyone agreed on (a rejoining player replaying) is checked alone.
      const agreed = this.agreed.get(msg.tick);
      if (agreed) {
        if (!agreed.has(m.player)) this.judgeHash(msg.tick, new Map([...agreed, [m.player, msg.checksum]]));
        return;
      }
      let at = this.hashes.get(msg.tick);
      if (!at) {
        at = new Map();
        this.hashes.set(msg.tick, at);
      }
      at.set(m.player, msg.checksum);
      const need =
        this.config?.slots.filter((s) => this.membersStill(s.player) && !this.mailbox?.isDropped(s.player)).length ??
        0;
      if (need > 0 && at.size >= need) this.judgeHash(msg.tick, at);
      return;
    }
    if (msg.type === "ended") {
      // A queue hears every seated player's report, also those after the room ended.
      if (this.onReport && this.config && this.state !== "desynced" && m.role === "player" && this.humanSlot(m.player))
        this.onReport({ player: m.player!, outcome: msg.outcome, tick: msg.tick });
      if (this.state !== "playing") return;
      this.shutdown(msg.outcome);
    }
  }

  /** The countdown ran out: the beats flow again. */
  private resumed() {
    const pause = this.pause;
    this.cancelResume = null;
    this.pause = null;
    if (!pause || this.state !== "playing" || !this.mailbox) return;
    // The waiting clock stood still while paused.
    this.lastProgress += this.now() - pause.at;
    this.log("resumed");
    this.fanout({ type: "resumed" });
    this.mailbox.setPaused(false);
  }

  /** How long the beats have waited (ms); 0 before `go` and while paused. */
  private waitedMs(): number {
    if (!this.started || this.pause || !this.mailbox) return 0;
    return this.now() - this.lastProgress;
  }

  /** Once a second: tells everyone whom the beats wait for, once they waited 5 s. */
  private noticeWaiting() {
    if (this.state !== "playing" || !this.mailbox || this.pause) return;
    const players = this.started ? this.mailbox.waitingFor() : [];
    const waited = this.waitedMs();
    if (players.length && waited >= WAITING_NOTICE_AFTER_MS) {
      this.waitingShown = true;
      this.fanout({ type: "waiting", players, seconds: Math.floor(waited / 1000) });
    } else if (this.waitingShown && !players.length) {
      this.waitingShown = false;
      this.fanout({ type: "waiting", players: [], seconds: 0 });
    }
  }

  private tryGo(){
    if(this.started||this.state!=='playing'||!this.contentAgreement)return;
    const required=this.config?.slots.filter(s=>this.membersStill(s.player))??[];
    if(!required.length||required.some(s=>!this.ready.has(s.player)))return;
    this.started=true;this.lastProgress=this.now();
    this.fanout({type:'go',tick:(this.mailbox?.tick??0)+1});this.mailbox?.releaseContentBarrier();
  }

  /** Terminal: spectators get the commits they have not had, then everyone `ended`; send
   * callbacks drop. MatchHost.discard deletes the row. */
  shutdown(outcome: WireOutcome = { winner: null, defeated: [] }): void {
    this.cancelResume?.();
    this.cancelResume = null;
    if (this.state !== "ended") {
      if (this.state === "playing") this.releaseSpectators(this.mailbox?.tick ?? 0);
      if (this.config) {
        const ids = [...this.seatIds].map(([p, id]) => `seat ${p} ${id}`).join(", ");
        this.log(`ended at tick ${this.mailbox?.tick ?? 0}: winner ${outcome.winner ?? "none"}, defeated [${outcome.defeated.join(", ")}]; ${ids}`);
      }
      this.state = "ended";
      this.replayId ??= crypto.randomUUID();
      this.fanout({ type: "ended", outcome, replayId: this.replayId });
    }
    for (const m of this.members.values()) m.send = null;
  }

  private membersStill(player: number): boolean {
    return [...this.members.values()].some(
      (m) => m.role === "player" && m.player === player && m.send,
    );
  }

  /** Transport maintenance and the waiting notices; never advances the game or changes a live delay. */
  pulse():void {
    if (this.queue && this.state === "waiting" && this.now() - this.queuedAt >= QUEUE_BIND_MS) this.cancel("a player did not connect within 30 s");
    const connected=[...this.members.values()].some(m=>m.role==='player'&&m.send);
    if(connected)this.emptySince=null;
    else this.emptySince??=this.now();
    if(this.state!=='waiting'&&this.state!=='playing')return;
    for(const m of this.members.values())this.probeMember(m);
    this.noticeWaiting();
  }

  /** Whether nobody can come back to this room any more: a finished or desynced match no
   * player is connected to, or a lobby or match without players for `grace` ms. */
  abandoned(grace:number):boolean {
    if(this.emptySince===null)return false;
    const finished=this.state==='ended'||this.state==='desynced';
    return finished||this.now()-this.emptySince>=grace;
  }
  private probeMember(m:Member){
    if(m.role!=='player'||!m.send)return;
    const id=m.latency.probe(this.now());
    if(id)m.send({type:'latencyProbe',id});
  }
  private chooseDelay(){
    return connectionDelay([...this.members.values()].filter(m=>m.role==='player').map(m=>m.latency.roundTrip(this.now())));
  }

  private judgeHash(tick: number, at: Map<number, number>): void {
    const hashes = [...at.entries()].map(([player, checksum]) => ({
      player,
      checksum,
    }));
    const first = hashes[0]?.checksum;
    if (first == null) return;
    if (hashes.every((h) => h.checksum === first)) {
      // Agreed once: a later matching hash only joins the agreement, silently.
      const fresh = !this.agreed.has(tick);
      this.agreed.set(tick, at);
      this.hashes.delete(tick);
      if (fresh) this.fanout({ type: "hashOk", tick });
      return;
    }
    this.state = "desynced";
    this.fanout({ type: "desync", tick, hashes });
  }

  private fanout(msg: ServerMsg): void {
    for (const m of this.members.values()) m.send?.(msg);
  }
}

/** How a member appears in the log: name and player id. */
function who(m: Member): string {
  return `${m.name} (${m.playerId})`;
}

/** How long a lobby or match without any connected player waits for one to come back. */
export const ABANDONED_AFTER_MS=120_000;

export class MatchHost {
  private readonly rooms = new Map<string, HostedMatch>();
  private nextId = 1;
  constructor(private readonly now=()=>performance.now(), private readonly options: HostOptions = {}) {}
  /** Every second: latency probes, waiting notices, and rooms nobody can come back to are discarded. */
  pulse(){
    for(const room of [...this.rooms.values()]){
      room.pulse();
      if(room.abandoned(ABANDONED_AFTER_MS))this.discard(room.id);
    }
  }

  create(draft: CreateRoom): {
    token: string;
    room: RoomView;
    you: ClientIdentity;
  } {
    const match = new HostedMatch(draft, String(this.nextId++),this.now,this.options);
    this.rooms.set(match.id, match);
    const you = match.you(match.hostToken)!;
    return { token: match.hostToken, room: match.view(), you };
  }

  /** A queue's room for two matched players; each seat's token and identity, as joining
   * would give them. */
  createQueued(setup: QueuedMatch): { room: HostedMatch; seats: [{ token: string; you: ClientIdentity }, { token: string; you: ClientIdentity }] } {
    const [first, second] = setup.seats;
    const draft: CreateRoom = {
      name: setup.mode === "ranked" ? "Ranked 1v1" : "Unranked 1v1",
      mapId: setup.mapId,
      mapRevision: setup.mapId,
      slotCount: 2,
      guestName: first.name,
      playerId: first.playerId,
    };
    const match = new HostedMatch(draft, String(this.nextId++), this.now, this.options);
    this.rooms.set(match.id, match);
    const tokens = match.queued(setup);
    const seat = (t: string) => ({ token: t, you: match.you(t)! });
    return { room: match, seats: [seat(tokens[0]), seat(tokens[1])] };
  }

  get(id: string): HostedMatch | undefined {
    return this.rooms.get(id);
  }

  /** Lobbies open to join and matches under way (to spectate); finished ones are not listed. */
  list(): RoomView[] {
    return [...this.rooms.values()]
      .map((r) => r.view())
      .filter((v) => v.state === "waiting" || v.state === "playing");
  }

  /** End every session, including ended/desynced rooms hidden from discovery. */
  discardAll(): number {
    const ids = [...this.rooms.keys()];
    for (const id of ids) this.discard(id);
    return ids.length;
  }

  /** Drop a finished / test room so the process does not leak HostedMatch forever. */
  discard(id: string): boolean {
    const room = this.rooms.get(id);
    if (!room) return false;
    room.shutdown();
    this.rooms.delete(id);
    return true;
  }
}

function token(): string {
  return crypto.randomUUID();
}

/** Eight hex digits: enough to tell a room's members apart, nothing to authenticate with. */
function shortId(): string {
  return crypto.randomUUID().slice(0, 8);
}

function seedU32(): number {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return buf[0]!;
}
