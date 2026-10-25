/**
 * The lockstep mailbox: collect `through` from every playing slot, emit `commit`. It keeps
 * every commit of the match (a returning player or a spectator catches up from them), takes
 * dropped seats out of the lockstep and back in (writing `away` and `back` into their slots),
 * and holds commits back while the match is paused (docs/server.md#protocol-2).
 */
import { validClientAction } from "./protocol";
import type {
  Action,
  Bundle,
  Commit,
  MatchConfig,
  PipelineSnap,
  ServerMsg,
} from "./protocol";

export class Room {
  private readonly through = new Map<number, number>();
  private readonly held = new Map<number, Map<number, Action[]>>();
  /** Last committed tick (0 before the first `commit`). */
  get tick(): number {
    return this.committed;
  }
  private committed = 0;
  /** The tick the log starts after: 0, or the saved tick a loaded match resumed from. */
  private base = 0;
  private readonly dropped = new Set<number>();
  /** `away` or `back` for a seat's slot of the next commit. */
  private readonly inserts = new Map<number, Action>();
  /** Every commit that holds an action, in tick order; the others were empty for every slot. */
  private readonly log: Commit[] = [];
  private paused = false;
  private readonly listeners: Array<(msg: ServerMsg) => void> = [];
  private readonly players: number[];

  /** Seats that send commands; computers run inside every player's game and never do. */
  private readonly humans: MatchConfig["slots"];

  constructor(readonly config: MatchConfig,private acceptingCommits=true) {
    this.humans = config.slots.filter((s) => s.kind === "human");
    this.players = config.slots.map((s) => s.player).sort((a, b) => a - b);
    for (const slot of this.humans) {
      this.through.set(slot.player, 0);
      this.held.set(slot.player, new Map());
    }
  }

  subscribe(fn: (msg: ServerMsg) => void): () => void {
    this.listeners.push(fn);
    return () => {
      const i = this.listeners.indexOf(fn);
      if (i >= 0) this.listeners.splice(i, 1);
    };
  }

  /** Freeze mailbox for a save. `commits` / `sentThrough` come from Lockstep. */
  snapshot(): Omit<PipelineSnap, "commits" | "sentThrough"> {
    const through: PipelineSnap["through"] = [];
    for (const [player, value] of this.through)
      through.push({ player, through: value });
    const held: PipelineSnap["held"] = [];
    for (const [player, map] of this.held) {
      for (const [tick, actions] of map)
        held.push({ player, tick, actions: actions.slice() });
    }
    return { committed: this.committed, through, held };
  }

  /** Resume after load. Does not re-emit already-committed ticks. */
  resume(snap: Pick<PipelineSnap, "committed" | "through" | "held">): void {
    this.committed = snap.committed;
    this.base = snap.committed;
    this.log.length = 0;
    this.through.clear();
    for (const slot of this.humans) this.through.set(slot.player, 0);
    for (const t of snap.through) this.through.set(t.player, t.through);
    this.held.clear();
    for (const slot of this.humans) this.held.set(slot.player, new Map());
    for (const h of snap.held) {
      const map = this.held.get(h.player) ?? new Map();
      this.held.set(h.player, map);
      map.set(h.tick, h.actions.slice());
    }
    this.dropped.clear();
    this.inserts.clear();
  }

  /** Slot confirms it will send no more actions with tick <= `through`. Bundles may be for through+D. */
  confirm(player: number, through: number, bundles: readonly Bundle[]): void {
    if (!this.through.has(player) || this.dropped.has(player)) return;
    if (
      !Number.isSafeInteger(through) ||
      through < 0 ||
      through > this.committed + 4096 ||
      !Array.isArray(bundles) ||
      bundles.length > 64
    )
      return;
    if (
      !bundles.every(
        (b) =>
          b &&
          Number.isSafeInteger(b.tick) &&
          b.tick > 0 &&
          b.tick <= through + this.config.delay + 1 &&
          Array.isArray(b.actions) &&
          b.actions.length <= 64 &&
          b.actions.every(validClientAction),
      )
    )
      return;
    if (through <= (this.through.get(player) ?? 0)) return;
    this.through.set(player, through);
    const held = this.held.get(player);
    if (!held) return;
    for (const b of bundles) {
      if (b.tick <= this.committed) continue;
      if (!held.has(b.tick)) held.set(b.tick, structuredClone(b.actions));
    }
    this.flush();
  }

  /** Takes a seat out of the lockstep: the beats stop waiting for it, its slot of the next
   * commit carries `away`, and what it sent for later beats is discarded. False if it is not
   * a human seat or is out already. */
  drop(player: number): boolean {
    if (!this.through.has(player) || this.dropped.has(player)) return false;
    this.dropped.add(player);
    this.held.get(player)?.clear();
    // Back and gone again before a beat committed: nothing to tell.
    if (this.inserts.get(player)?.type === "back") this.inserts.delete(player);
    else this.inserts.set(player, { type: "away" });
    this.flush();
    return true;
  }

  /** A returning player who caught up: its `through` counts from the last commit and nothing
   * it sent before is kept. A dropped seat is taken back into the lockstep at the next beat,
   * whose slot carries `back`. Returns whether the seat had been dropped. */
  rejoin(player: number): boolean {
    if (!this.through.has(player)) return false;
    this.through.set(player, this.committed);
    this.held.get(player)?.clear();
    if (!this.dropped.has(player)) return false;
    this.dropped.delete(player);
    if (this.inserts.get(player)?.type === "away") this.inserts.delete(player);
    else this.inserts.set(player, { type: "back" });
    return true;
  }

  isDropped(player: number): boolean {
    return this.dropped.has(player);
  }

  /** The human seats the next beat waits for; none before `go` or while paused. */
  waitingFor(): number[] {
    if (!this.acceptingCommits || this.paused) return [];
    const next = this.committed + 1;
    return this.humans
      .map((s) => s.player)
      .filter((p) => !this.dropped.has(p) && (this.through.get(p) ?? 0) < next)
      .sort((a, b) => a - b);
  }

  /** While paused no beat commits; confirmations are still collected. */
  setPaused(paused: boolean): void {
    this.paused = paused;
    this.flush();
  }

  /** Every commit holding an action with `since < tick <= through`. */
  history(since: number, through: number): Commit[] {
    const out: Commit[] = [];
    for (let i = this.firstAfter(since); i < this.log.length; i++) {
      const c = this.log[i]!;
      if (c.tick > through) break;
      out.push(c);
    }
    return out;
  }

  /** The commit of `tick` (at most the last committed one), empty slots if it held nothing. */
  commitAt(tick: number): Commit {
    const c = this.log[this.firstAfter(tick - 1)];
    if (c && c.tick === tick) return c;
    return { tick, slots: this.players.map((player) => ({ player, actions: [] })) };
  }

  /** The log's first index whose tick is after `tick`. */
  private firstAfter(tick: number): number {
    let lo = 0, hi = this.log.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.log[mid]!.tick <= tick) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** The tick the kept commits start after. */
  get logBase(): number {
    return this.base;
  }

  releaseContentBarrier(){this.acceptingCommits=true;this.flush();}

  private flush(): void {
    if(!this.acceptingCommits||this.paused)return;
    while (this.ready(this.committed + 1)) {
      const tick = this.committed + 1;
      let any = false;
      const slots = this.players.map((player) => {
        const held = this.held.get(player)?.get(tick) ?? [];
        const insert = this.inserts.get(player);
        const actions = insert ? [insert, ...held] : held;
        if (actions.length) any = true;
        return { player, actions };
      });
      for (const held of this.held.values()) held.delete(tick);
      this.inserts.clear();
      this.committed = tick;
      if (any) this.log.push({ tick, slots });
      const msg: ServerMsg = { type: "commit", tick, slots };
      for (const fn of this.listeners) fn(msg);
    }
  }

  private ready(tick: number): boolean {
    for (const slot of this.humans) {
      if (this.dropped.has(slot.player)) continue;
      if ((this.through.get(slot.player) ?? 0) < tick) return false;
    }
    return this.humans.some((s) => !this.dropped.has(s.player));
  }
}
